//! The crate's entry point. Owns config, history, and providers; opens sessions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::audio::encode_wav_pcm16;
use crate::backup::{self, BackupError, BackupInfo, Categories, ImportReport, Scope, WriteReport};
use crate::config::{ConfigStore, FirstRunDefaults, ProviderConfig, ProviderKind, StoreError};
use crate::device::Device;
use crate::history::{DeviceRef, DeviceRow, HistoryError, HistoryStore, MoveReport, NewUtterance};
use crate::paths::DataDir;
use crate::pipeline::Pipeline;
use crate::profile::resolve;
use crate::protocol::{Event, PROTOCOL_VERSION};
use crate::providers::factory::ProviderFactory;
use crate::providers::probe::{probe_http, ProbeFailure, ProbeResult};
use crate::providers::registry::ProviderRegistry;
use crate::secrets::SecretStore;
use crate::session::{Session, SessionCmd, SessionFlags};
use crate::steps::PipelineContext;
use crate::types::{AppInfo, AudioClip, Context, ErrorCode, HistoryId, ProfileId, SessionId};
use crate::SAMPLE_RATE;

/// Shorter utterances are rejected before any provider call.
pub const MIN_UTTERANCE_SECS: f32 = 0.2;
const EVENT_BUFFER: usize = 256;
const PRUNE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(86_400);

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown session `{0}`")]
    UnknownSession(SessionId),
    #[error("a dictation is being processed")]
    Busy,
    #[error("the folder cannot be used: {0}")]
    Folder(String),
    #[error(transparent)]
    Backup(#[from] BackupError),
    #[error(transparent)]
    Settings(#[from] crate::settings_file::SettingsFileError),
}

/// Where a backup is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupSource {
    /// One archive file.
    File(PathBuf),
    /// The tree itself.
    Folder(PathBuf),
}

#[derive(Debug, thiserror::Error)]
pub enum ReprocessError {
    #[error("the history row no longer exists")]
    NoRow,
    #[error("the row has no recording")]
    NoRecording,
    #[error(transparent)]
    History(#[from] HistoryError),
    /// The engines could not produce a result; the text says why.
    #[error("{text}")]
    Failed { code: ErrorCode, text: String },
}

/// The result of `Engine::reprocess` and the engines that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reprocessed {
    pub redo: crate::history::Redo,
    pub asr_label: String,
    /// None when the profile does not rewrite.
    pub llm_label: Option<String>,
}

/// Rearms the idle unload of the language model when a rerun ends.
struct RearmIdle<'a>(&'a Engine);

impl Drop for RearmIdle<'_> {
    fn drop(&mut self) {
        self.0.touch_idle();
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionRequest {
    pub app: AppInfo,
    /// Already filtered by the client's per-field opt-ins.
    pub context: Context,
    pub profile_override: Option<ProfileId>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    dirs: DataDir,
    device: Mutex<Device>,
    app_version: String,
    config: ConfigStore,
    history: Arc<HistoryStore>,
    registry: ProviderRegistry,
    client: reqwest::Client,
    secrets: Arc<dyn SecretStore>,
    sessions: Mutex<HashMap<SessionId, HistoryId>>,
    /// Recordings of dictations still being processed. Their history rows
    /// say "did not finish" until the result replaces them.
    in_flight: Mutex<HashSet<PathBuf>>,
    /// Used by `start_session` so it can be called from a non-runtime thread
    /// (for example a platform hotkey callback) rather than only from
    /// inside an `async fn` already running on this runtime.
    runtime: tokio::runtime::Handle,
    /// Bumped whenever a session starts or finishes, so a pending idle
    /// unload only fires if nothing happened since it was scheduled.
    idle_generation: AtomicU64,
    /// Test-only hook: when greater than zero, `run_session` sleeps this
    /// many milliseconds just before the history insert, so a test can
    /// deterministically land a `cancel()` inside the non-abortable tail
    /// instead of racing real I/O timing.
    #[cfg(test)]
    pub(crate) tail_delay_ms: AtomicU64,
}

/// Everything a client supplies when it opens the engine.
pub struct EngineOptions {
    pub secrets: Arc<dyn SecretStore>,
    pub factories: Vec<Arc<dyn ProviderFactory>>,
    pub first_run: FirstRunDefaults,
    pub identity: DeviceOptions,
}

/// What the client knows about the machine the engine runs on.
#[derive(Debug, Clone)]
pub struct DeviceOptions {
    /// The machine's own id, when the platform gives one. It tells a data
    /// directory that was copied to another machine from the original.
    pub hardware_id: Option<String>,
    /// The name a new installation starts with.
    pub name: String,
    pub platform: String,
    /// The client's version, written into backups.
    pub app_version: String,
}

impl Default for DeviceOptions {
    fn default() -> Self {
        Self {
            hardware_id: None,
            name: "This device".into(),
            platform: std::env::consts::OS.into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

impl Engine {
    pub async fn open(dirs: DataDir, secrets: Arc<dyn SecretStore>) -> Result<Engine, EngineError> {
        Self::open_with(
            dirs,
            EngineOptions {
                secrets,
                factories: Vec::new(),
                first_run: FirstRunDefaults::default(),
                identity: DeviceOptions::default(),
            },
        )
        .await
    }

    pub async fn open_with(dirs: DataDir, opts: EngineOptions) -> Result<Engine, EngineError> {
        let runtime = tokio::runtime::Handle::current();
        dirs.ensure()?;
        let config = ConfigStore::open_with_defaults(&dirs, &opts.first_run)?;
        let path = dirs.history_path();
        let general = config.current().general.clone();
        let recordings = general
            .recordings_dir
            .clone()
            .unwrap_or_else(|| dirs.recordings_dir());
        // One identity per database: a data directory that lost its database
        // starts another one, so its rows never share ids with the old ones.
        let identity = if path.exists() {
            Device::load_or_create
        } else {
            Device::renew
        };
        let device = identity(
            &dirs.data_dir,
            opts.identity.hardware_id.as_deref(),
            &opts.identity.name,
        )?;
        let history_recordings = recordings.clone();
        let opened = device.clone();
        let platform = opts.identity.platform.clone();
        let history = Arc::new(
            tokio::task::spawn_blocking(move || {
                let device = DeviceRef {
                    id: &opened.id,
                    name: &opened.name,
                    platform: &platform,
                };
                HistoryStore::open(&path, &history_recordings, &device)
            })
            .await
            .expect("join")?,
        );

        // Earlier builds kept recordings in the cache directory. They are the
        // user's data: they move next to the history, once.
        let legacy = dirs.legacy_recordings_dir();
        if general.recordings_dir.is_none() && legacy != recordings && legacy.is_dir() {
            let h = history.clone();
            let to = recordings.clone();
            let moved = tokio::task::spawn_blocking(move || {
                let report = h.move_recordings(&legacy, &to);
                // Goes only when nothing is left in it.
                let _ = std::fs::remove_dir(&legacy);
                report
            })
            .await;
            match moved {
                Ok(Ok(report)) if report.moved + report.left > 0 => tracing::info!(
                    moved = report.moved,
                    left = report.left,
                    "recordings moved out of the cache directory"
                ),
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "recordings could not be moved"),
                Err(e) => tracing::warn!(error = %e, "recordings move task failed"),
            }
        }

        let h = history.clone();
        // Best effort: a prune failure (or panic) must not prevent opening
        // the engine, so its result is discarded rather than propagated.
        let _ = tokio::task::spawn_blocking(move || {
            h.prune(
                general.history_retention_days,
                general.recording_retention_days,
                &recordings,
            )
        })
        .await;

        let client = reqwest::Client::builder().build().expect("reqwest client");
        let registry =
            ProviderRegistry::with_factories(client.clone(), opts.secrets.clone(), opts.factories);
        let engine = Engine {
            inner: Arc::new(Inner {
                dirs,
                device: Mutex::new(device),
                app_version: opts.identity.app_version,
                config,
                history,
                registry,
                client,
                secrets: opts.secrets,
                sessions: Mutex::new(HashMap::new()),
                in_flight: Mutex::new(HashSet::new()),
                runtime,
                idle_generation: AtomicU64::new(0),
                #[cfg(test)]
                tail_delay_ms: AtomicU64::new(0),
            }),
        };
        engine.spawn_daily_prune();
        Ok(engine)
    }

    /// Applies the retention settings once a day for as long as the engine
    /// lives, so a process that stays up for weeks still expires old entries.
    fn spawn_daily_prune(&self) {
        let weak = Arc::downgrade(&self.inner);
        self.inner.runtime.spawn(async move {
            let mut tick = tokio::time::interval(PRUNE_INTERVAL);
            // The first tick fires at once; `open_with` has just pruned.
            tick.tick().await;
            loop {
                tick.tick().await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let general = inner.config.current().general.clone();
                let recordings = inner.recordings_dir();
                let history = inner.history.clone();
                drop(inner);
                let pruned = tokio::task::spawn_blocking(move || {
                    history.prune(
                        general.history_retention_days,
                        general.recording_retention_days,
                        &recordings,
                    )
                })
                .await;
                match pruned {
                    Ok(Ok(report)) => tracing::info!(
                        rows = report.rows_deleted,
                        recordings = report.recordings_deleted,
                        "retention applied"
                    ),
                    Ok(Err(e)) => tracing::warn!(error = %e, "retention failed"),
                    Err(e) => tracing::warn!(error = %e, "retention task failed"),
                }
            }
        });
    }

    /// Loads the default profile's ASR provider in the background so the
    /// first dictation does not pay the model load. Failures are ignored:
    /// the session reports them when it needs the provider.
    pub fn warm_up_default_asr(&self) {
        let inner = self.inner.clone();
        self.inner.runtime.spawn(async move {
            let config = inner.config.current();
            let Some(profile) = config.profiles.iter().find(|p| p.is_default) else {
                return;
            };
            let Some(cfg) = config.provider(&profile.asr_provider) else {
                return;
            };
            if let Ok(p) = inner.registry.asr(cfg, &config.general) {
                let _ = p.load().await;
            }
        });
    }

    pub fn config(&self) -> &ConfigStore {
        &self.inner.config
    }

    pub(crate) fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.inner.secrets
    }

    pub fn history(&self) -> Arc<HistoryStore> {
        self.inner.history.clone()
    }

    pub fn dirs(&self) -> &DataDir {
        &self.inner.dirs
    }

    /// The identity of this installation.
    pub fn device(&self) -> Device {
        self.inner.device.lock().unwrap().clone()
    }

    /// The devices whose rows are in the history: this one first.
    pub fn devices(&self) -> Result<Vec<DeviceRow>, EngineError> {
        Ok(self.inner.history.devices()?)
    }

    /// Gives a device another name. The name of this one is also what its
    /// backups carry.
    pub async fn rename_device(&self, id: &str, name: &str) -> Result<(), EngineError> {
        let name = if id == self.inner.history.device_id() {
            let renamed = Device::rename(&self.inner.dirs.data_dir, name)?;
            let name = renamed.name.clone();
            *self.inner.device.lock().unwrap() = renamed;
            name
        } else {
            let name: String = name.trim().chars().take(crate::device::NAME_MAX).collect();
            if name.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "the name is empty",
                )
                .into());
            }
            name
        };
        self.inner.history.set_device_name(id, &name)?;
        Ok(())
    }

    /// Removes another device's history rows, recordings and statistics.
    pub async fn remove_device(&self, id: &str) -> Result<usize, EngineError> {
        let history = self.inner.history.clone();
        let recordings = self.inner.recordings_dir();
        let id = id.to_string();
        Ok(
            tokio::task::spawn_blocking(move || history.remove_device(&id, &recordings))
                .await
                .expect("join")?,
        )
    }

    /// Whether a dictation is being processed.
    pub fn busy(&self) -> bool {
        !self.inner.in_flight.lock().unwrap().is_empty()
    }

    /// Brings the backup folder at `dir` up to date with this device's rows.
    /// What is in the folder already is never removed.
    pub async fn backup_write_folder(
        &self,
        dir: &Path,
        cats: Categories,
        settings: Option<Vec<u8>>,
    ) -> Result<WriteReport, EngineError> {
        // A folder that is gone (a volume that is not mounted, a folder that
        // was removed) is reported, not made again somewhere else.
        if !dir.is_dir() {
            return Err(BackupError::Io {
                path: dir.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the folder does not exist",
                ),
            }
            .into());
        }
        self.write_backup(dir.to_path_buf(), cats, Scope::ThisDevice, settings)
            .await
    }

    /// Writes one archive file with the rows of every device here.
    pub async fn backup_export_file(
        &self,
        file: &Path,
        cats: Categories,
        settings: Option<Vec<u8>>,
    ) -> Result<WriteReport, EngineError> {
        let staging = tempfile::tempdir_in(&self.inner.dirs.cache_dir)?;
        let tree = staging.path().join("tree");
        let report = self
            .write_backup(tree.clone(), cats, Scope::AllDevices, settings)
            .await?;
        let file = file.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let packed = backup::archive::pack(&tree, &file);
            drop(staging);
            packed
        })
        .await
        .expect("join")?;
        Ok(report)
    }

    async fn write_backup(
        &self,
        root: PathBuf,
        cats: Categories,
        scope: Scope,
        settings: Option<Vec<u8>>,
    ) -> Result<WriteReport, EngineError> {
        let inner = self.inner.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let recordings = inner.recordings_dir();
            let in_use = |p: &Path| inner.in_flight.lock().unwrap().contains(p);
            backup::write_tree(
                &inner.history,
                &root,
                cats,
                scope,
                &recordings,
                &inner.app_version,
                settings.as_deref(),
                &in_use,
            )
        })
        .await
        .expect("join")?)
    }

    /// The tree of a source: the folder itself, or the archive unpacked
    /// into a directory that lives as long as the returned guard.
    fn open_source(
        cache: &Path,
        src: &BackupSource,
    ) -> Result<(PathBuf, Option<tempfile::TempDir>), BackupError> {
        match src {
            BackupSource::Folder(dir) => Ok((dir.clone(), None)),
            BackupSource::File(file) => {
                let staging = tempfile::tempdir_in(cache).map_err(|e| BackupError::Io {
                    path: cache.to_path_buf(),
                    source: e,
                })?;
                backup::archive::unpack(file, staging.path())?;
                Ok((staging.path().to_path_buf(), Some(staging)))
            }
        }
    }

    /// What a backup holds.
    pub async fn backup_inspect(&self, src: &BackupSource) -> Result<BackupInfo, EngineError> {
        let inner = self.inner.clone();
        let src = src.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let (root, _staging) = Self::open_source(&inner.dirs.cache_dir, &src)?;
            backup::inspect(&root, inner.history.device_id())
        })
        .await
        .expect("join")?)
    }

    /// Merges the named devices of a backup into the history. Refused while
    /// a dictation is being processed.
    pub async fn backup_import(
        &self,
        src: &BackupSource,
        devices: Vec<String>,
        cats: Categories,
    ) -> Result<ImportReport, EngineError> {
        if self.busy() {
            return Err(EngineError::Busy);
        }
        let inner = self.inner.clone();
        let src = src.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let (root, _staging) = Self::open_source(&inner.dirs.cache_dir, &src)?;
            let retention = inner.config.current().general.history_retention_days;
            backup::import_tree(
                &inner.history,
                &root,
                &devices,
                cats,
                &inner.recordings_dir(),
                retention,
            )
        })
        .await
        .expect("join")?)
    }

    /// The folder recordings are kept in: the one the user chose, or the
    /// default one.
    pub fn recordings_dir(&self) -> PathBuf {
        self.inner.recordings_dir()
    }

    /// Whether `path` is the recording of a dictation still being processed.
    pub fn in_flight(&self, path: &Path) -> bool {
        self.inner.in_flight.lock().unwrap().contains(path)
    }

    /// Keeps recordings in `dir` from now on, or in the default folder for
    /// `None`, and moves the existing ones there.
    pub async fn set_recordings_dir(
        &self,
        dir: Option<PathBuf>,
    ) -> Result<MoveReport, EngineError> {
        if !self.inner.in_flight.lock().unwrap().is_empty() {
            return Err(EngineError::Busy);
        }
        let default = self.inner.dirs.recordings_dir();
        let target = dir.clone().unwrap_or_else(|| default.clone());
        if !target.is_absolute() {
            return Err(EngineError::Folder("not an absolute path".into()));
        }
        let old = self.inner.recordings_dir();
        let history = self.inner.history.clone();
        let to = target.clone();
        let report = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&to).map_err(|e| EngineError::Folder(e.to_string()))?;
            // A folder that cannot be written to is refused before anything moves.
            let probe = to.join(".arcvox-write-test");
            std::fs::write(&probe, b"").map_err(|e| EngineError::Folder(e.to_string()))?;
            let _ = std::fs::remove_file(&probe);
            Ok::<_, EngineError>(history.move_recordings(&old, &to)?)
        })
        .await
        .expect("join")?;
        let setting = (target != default).then_some(target);
        self.inner
            .config
            .update(|c| c.general.recordings_dir = setting)?;
        Ok(report)
    }

    /// Whether this build can make engines of this kind.
    pub(crate) fn has_factory(&self, kind: ProviderKind) -> bool {
        self.inner.registry.factory_for(kind).is_some()
    }

    pub async fn probe_provider(&self, cfg: &ProviderConfig) -> ProbeResult {
        match cfg.kind {
            ProviderKind::Http => probe_http(&self.inner.client, cfg, &self.inner.secrets).await,
            kind => match self.inner.registry.factory_for(kind) {
                Some(f) => f.probe(cfg).await,
                None => ProbeResult {
                    ok: false,
                    failure: Some(ProbeFailure::Other {
                        message: "provider kind not available in this build".into(),
                    }),
                },
            },
        }
    }

    /// May be called from a thread that is not running on this engine's
    /// tokio runtime (the task is spawned via the captured `Handle`).
    pub fn start_session(&self, req: SessionRequest) -> Session {
        // Bumped here rather than inside the spawned task, so a session
        // cancelled before its task first runs still cancels a pending
        // unload from an earlier session.
        self.inner.idle_generation.fetch_add(1, Ordering::SeqCst);
        let config = self.inner.config.current();
        let profile =
            Arc::new(resolve(&config.profiles, &req.app, req.profile_override.as_ref()).clone());
        let id = SessionId(uuid::Uuid::new_v4().to_string());
        let (events_tx, events_rx) = mpsc::channel(EVENT_BUFFER);
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let wav_path = self.inner.recordings_dir().join(format!("{id}.wav"));
        let audio = Arc::new(Mutex::new(Vec::<f32>::new()));
        let flags = Arc::new(SessionFlags::default());

        let _ = events_tx.try_send(Event::SessionStarted {
            session_id: id.clone(),
            profile_id: profile.id.clone(),
            protocol_version: PROTOCOL_VERSION,
        });

        let task = self.inner.runtime.spawn(run_session(
            self.inner.clone(),
            id.clone(),
            profile.clone(),
            req,
            audio.clone(),
            cmd_rx,
            wav_path.clone(),
            events_tx.clone(),
            flags.clone(),
        ));

        Session {
            id,
            profile_id: profile.id.clone(),
            audio,
            cmd_tx: Mutex::new(Some(cmd_tx)),
            events_tx,
            events_rx: Mutex::new(Some(events_rx)),
            task: Mutex::new(Some(task)),
            flags,
            on_cancel_done: {
                let inner = self.inner.clone();
                Arc::new(move || {
                    inner.schedule_idle_unload();
                    inner.discard_held(wav_path.clone());
                })
            },
        }
    }

    /// Runs the recording of a history row through a profile again and
    /// returns the result without storing it. `profile` names the profile to
    /// use; without it, or when it no longer exists, the row's own profile
    /// is used, and the default one when that is gone too. The profile's
    /// current engines and settings apply, with the application name as the
    /// only context, since nothing else of the original moment was kept.
    pub async fn reprocess(
        &self,
        id: HistoryId,
        profile: Option<ProfileId>,
    ) -> Result<Reprocessed, ReprocessError> {
        let history = self.inner.history.clone();
        let recordings = self.inner.recordings_dir();
        let (row, clip) = tokio::task::spawn_blocking(move || {
            let row = history.get(id)?.ok_or(ReprocessError::NoRow)?;
            let path = history
                .recording_path(id, &recordings)?
                .ok_or(ReprocessError::NoRecording)?;
            let bytes = std::fs::read(&path).map_err(|_| ReprocessError::NoRecording)?;
            let clip =
                crate::audio::decode_wav_pcm16(&bytes).map_err(|_| ReprocessError::NoRecording)?;
            Ok::<_, ReprocessError>((row, clip))
        })
        .await
        .map_err(|e| ReprocessError::Failed {
            code: ErrorCode::Internal,
            text: e.to_string(),
        })??;

        let config = self.inner.config.current();
        let wanted = profile.unwrap_or_else(|| row.profile_id.clone());
        let profile =
            Arc::new(resolve(&config.profiles, &AppInfo::default(), Some(&wanted)).clone());
        let label = |id: &crate::types::ProviderId| {
            config
                .provider(id)
                .map(|p| p.label.clone())
                .unwrap_or_else(|| id.0.clone())
        };
        let asr_label = label(&profile.asr_provider);
        let llm_label = profile
            .steps
            .iter()
            .any(|s| matches!(s, crate::config::StepSpec::Llm))
            .then(|| profile.llm_provider.as_ref().map(label))
            .flatten();

        self.mark_busy();
        let _idle = RearmIdle(self);
        let fail = |e: &crate::steps::StepError| ReprocessError::Failed {
            code: e.code,
            text: failure_text(e),
        };
        let pipeline =
            Pipeline::build(&config, &profile, &self.inner.registry).map_err(|e| fail(&e))?;
        let ctx =
            PipelineContext {
                profile: profile.clone(),
                context: Context {
                    app_name: row.app_name.clone(),
                    ..Context::default()
                },
                dictionary: Arc::new(config.dictionary.clone()),
                base_prompt: self.inner.config.base_prompt().unwrap_or_else(|_| {
                    crate::config::base_prompt::DEFAULT_BASE_PROMPT.to_string()
                }),
                general: config.general.clone(),
            };
        // Nobody listens to the step events of a rerun.
        let (tx, mut rx) = mpsc::channel(EVENT_BUFFER);
        let drain = self
            .inner
            .runtime
            .spawn(async move { while rx.recv().await.is_some() {} });
        let outcome = pipeline.run(Arc::new(clip), &ctx, &tx).await;
        drop(tx);
        let _ = drain.await;
        let outcome = outcome.map_err(|e| fail(&e))?;
        Ok(Reprocessed {
            asr_label,
            llm_label,
            redo: crate::history::Redo {
                profile_id: profile.id.clone(),
                raw_text: outcome.raw_text,
                final_text: outcome.final_text,
                degraded: outcome.degraded,
                asr_ms: outcome.asr_ms,
                llm_ms: outcome.llm_ms,
                dict_fixes: outcome.dict_fixes,
                snippets: outcome.snippets,
                llm_changed: outcome.llm_changed,
            },
        })
    }

    /// Records what the user actually inserted for a finished session.
    pub async fn record_final(
        &self,
        session_id: &SessionId,
        text: &str,
    ) -> Result<(), EngineError> {
        let history_id = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .copied()
            .ok_or_else(|| EngineError::UnknownSession(session_id.clone()))?;
        let history = self.inner.history.clone();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || history.set_final(history_id, &text))
            .await
            .expect("join")?;
        Ok(())
    }

    pub(crate) fn registry(&self) -> &ProviderRegistry {
        &self.inner.registry
    }

    /// Counts as activity for the idle LLM unload, so a timer armed earlier
    /// cannot fire while the caller is using a provider.
    pub(crate) fn mark_busy(&self) {
        self.inner.idle_generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Counts as activity for the idle LLM unload and rearms its timer.
    pub(crate) fn touch_idle(&self) {
        self.inner.idle_generation.fetch_add(1, Ordering::SeqCst);
        self.inner.schedule_idle_unload();
    }

    /// Drops and unloads every cached speech provider no profile uses, so a
    /// model loaded only to be timed does not stay in memory.
    pub async fn unload_unreferenced_asr(&self) {
        let config = self.inner.config.current();
        let evicted = self
            .inner
            .registry
            .evict_asr_where(|cfg| !config.profiles.iter().any(|p| p.asr_provider == cfg.id));
        for p in evicted {
            p.unload().await;
        }
    }

    /// Drops every cached provider that loads the model `id` and unloads it,
    /// so a deleted file frees its memory now and the next session reports
    /// the missing model instead of using the copy still in memory.
    pub async fn evict_model(&self, id: &str) {
        let (asr, llm) = self
            .inner
            .registry
            .evict_where(model_predicate(&self.inner.dirs, id));
        for p in asr {
            p.unload().await;
        }
        for p in llm {
            p.unload().await;
        }
    }
}

/// Which embedded provider configs load the model `id` names: a catalogue id,
/// or `custom/<file>` for an imported file under the models directory.
pub(crate) fn model_predicate(dirs: &DataDir, id: &str) -> impl Fn(&ProviderConfig) -> bool {
    let custom_dir = dirs.models_dir().join("custom");
    let custom_name = id.strip_prefix("custom/").map(str::to_string);
    let id = id.to_string();
    move |cfg| {
        if cfg.kind != ProviderKind::Embedded {
            return false;
        }
        match &custom_name {
            Some(name) => cfg.model_path.as_deref().is_some_and(|p| {
                p.parent() == Some(custom_dir.as_path())
                    && p.file_name().is_some_and(|f| f == name.as_str())
            }),
            None => cfg.model_id.as_deref() == Some(id.as_str()),
        }
    }
}

impl Inner {
    fn recordings_dir(&self) -> PathBuf {
        self.config
            .current()
            .general
            .recordings_dir
            .clone()
            .unwrap_or_else(|| self.dirs.recordings_dir())
    }

    /// Removes the recording kept for a cancelled dictation, with its row.
    /// A row that already holds a result or a failure of its own stays.
    fn discard_held(self: &Arc<Self>, path: PathBuf) {
        let inner = self.clone();
        self.runtime.spawn_blocking(move || {
            if matches!(inner.history.discard_interrupted(&path), Ok(true)) {
                let _ = std::fs::remove_file(&path);
            }
            inner.in_flight.lock().unwrap().remove(&path);
        });
    }

    /// Arms a timer that unloads every cached LLM provider once the engine
    /// has been idle for `llm_idle_unload_secs`. Zero disables it.
    fn schedule_idle_unload(self: &Arc<Self>) {
        let secs = self.config.current().general.llm_idle_unload_secs;
        if secs == 0 {
            return;
        }
        let generation = self.idle_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let inner = self.clone();
        self.runtime.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            if inner.idle_generation.load(Ordering::SeqCst) == generation {
                inner.unload_idle_llms().await;
            }
        });
    }

    pub(crate) async fn unload_idle_llms(&self) {
        for p in self.registry.llm_providers() {
            p.unload().await;
        }
    }
}

/// Sends `Done` at most once per session: whichever of `run_session` or
/// `Session::cancel` gets here first wins, and the other is a no-op. This is
/// what keeps a cancel racing the pipeline's tail from producing a second
/// `Done` alongside the one `cancel` already sent.
async fn send_done_once(
    tx: &mpsc::Sender<Event>,
    done_sent: &AtomicBool,
    session_id: &SessionId,
    history_id: Option<HistoryId>,
) {
    if !done_sent.swap(true, Ordering::SeqCst) {
        let _ = tx
            .send(Event::Done {
                session_id: session_id.clone(),
                history_id,
            })
            .await;
    }
}

// Folding done_sent/cancelled/tail_started into `SessionFlags` (one
// parameter instead of three) brings this down from 9 to 8 arguments, still
// one over clippy's default threshold of 7; every remaining parameter is a
// distinct, necessary piece of state, so the lint stays silenced rather than
// inventing a further grouping not called for elsewhere.
/// Why a dictation failed, as stored with its recording.
fn failure_text(err: &crate::steps::StepError) -> String {
    match &err.hint {
        Some(hint) => format!("{}; {hint}", err.message),
        None => err.message.clone(),
    }
}

/// What the row of a kept recording says until the dictation has a result.
const INTERRUPTED: &str = "the dictation did not finish";

/// The length of a clip in milliseconds, when its rate is known.
fn clip_ms(clip: &AudioClip) -> Option<u64> {
    (clip.sample_rate > 0).then(|| clip.samples.len() as u64 * 1000 / u64::from(clip.sample_rate))
}

/// Marks a recording as belonging to a dictation in progress for as long as
/// it lives, also when the session task is aborted.
struct InFlight {
    inner: Arc<Inner>,
    path: PathBuf,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.inner.in_flight.lock().unwrap().remove(&self.path);
    }
}

/// Writes the recording as it is, with a row that says the dictation did
/// not finish, before anything is done with the audio. Only when recordings
/// are kept. Returns the row, which the result replaces later.
async fn hold(
    inner: &Arc<Inner>,
    profile: &crate::config::Profile,
    req: &SessionRequest,
    audio: &Mutex<Vec<f32>>,
    path: &Path,
    flags: &Arc<SessionFlags>,
) -> Option<HistoryId> {
    if !inner.config.current().general.keep_recordings {
        return None;
    }
    let samples = audio.lock().unwrap().clone();
    if samples.is_empty() {
        return None;
    }
    let clip = AudioClip {
        samples,
        sample_rate: SAMPLE_RATE,
    };
    let row = crate::history::FailedUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        audio_path: path.to_path_buf(),
        audio_ms: clip_ms(&clip),
        code: ErrorCode::Interrupted,
        error: INTERRUPTED.into(),
    };
    inner.in_flight.lock().unwrap().insert(path.to_path_buf());
    let history = inner.history.clone();
    let flags = flags.clone();
    let p = path.to_path_buf();
    let stored = tokio::task::spawn_blocking(move || {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&p, encode_wav_pcm16(&clip))?;
        let id = history.insert_failed(row).inspect_err(|_| {
            let _ = std::fs::remove_file(&p);
        })?;
        // A cancel that came while this ran found nothing to remove.
        if flags.cancelled.load(Ordering::SeqCst) {
            let _ = history.delete(id);
            let _ = std::fs::remove_file(&p);
            return Ok(None);
        }
        Ok::<_, HistoryError>(Some(id))
    })
    .await;
    match stored {
        Ok(Ok(id)) => id,
        _ => {
            tracing::warn!(path = %path.display(), "the recording could not be kept before processing");
            None
        }
    }
}

/// The kept recording becomes the part that is processed. When this fails
/// the whole recording stays.
async fn replace_held(inner: &Arc<Inner>, id: HistoryId, path: &Path, clip: &AudioClip) {
    let bytes = encode_wav_pcm16(clip);
    let audio_ms = clip_ms(clip);
    let history = inner.history.clone();
    let p = path.to_path_buf();
    let replaced = tokio::task::spawn_blocking(move || {
        // Written beside it and renamed over it, so there is a whole file at every moment.
        let part = p.with_extension("part.wav");
        std::fs::write(&part, bytes)?;
        std::fs::rename(&part, &p).inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })?;
        history.set_audio_ms(id, audio_ms)
    })
    .await;
    if !matches!(replaced, Ok(Ok(()))) {
        tracing::warn!(path = %path.display(), "the kept recording was left untrimmed");
    }
}

/// Removes a kept recording and its row: nothing was said, or it was cancelled.
async fn drop_held(inner: &Arc<Inner>, id: HistoryId, path: &Path) {
    let history = inner.history.clone();
    let p = path.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        let _ = history.delete(id);
        let _ = std::fs::remove_file(&p);
    })
    .await;
}

/// Keeps the recording of a dictation that failed, with a history row that
/// says why, so it can be processed again. Only when recordings are kept;
/// silence and a cancelled dictation leave nothing. `held` is the row that
/// already keeps the recording.
#[allow(clippy::too_many_arguments)]
async fn keep_failed(
    inner: &Arc<Inner>,
    config: &crate::config::Config,
    profile: &crate::config::Profile,
    req: &SessionRequest,
    clip: &AudioClip,
    err: &crate::steps::StepError,
    flags: &SessionFlags,
    held: Option<HistoryId>,
    path: &Path,
) {
    let nothing_to_keep = matches!(err.code, ErrorCode::NoSpeech | ErrorCode::NoAudio)
        || flags.cancelled.load(Ordering::SeqCst);
    if let Some(id) = held {
        if nothing_to_keep {
            drop_held(inner, id, path).await;
        } else {
            let history = inner.history.clone();
            let (code, text) = (err.code, failure_text(err));
            let named =
                tokio::task::spawn_blocking(move || history.set_error(id, code, &text)).await;
            if !matches!(named, Ok(Ok(()))) {
                tracing::warn!(path = %path.display(), "the reason of the failure could not be stored");
            }
        }
        return;
    }
    if !config.general.keep_recordings || nothing_to_keep {
        return;
    }
    let path = path.to_path_buf();
    let bytes = encode_wav_pcm16(clip);
    let failed = crate::history::FailedUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        audio_path: path.clone(),
        audio_ms: clip_ms(clip),
        code: err.code,
        error: failure_text(err),
    };
    let history = inner.history.clone();
    let shown = path.clone();
    let stored = tokio::task::spawn_blocking(move || {
        std::fs::write(&path, bytes)?;
        history.insert_failed(failed).inspect_err(|_| {
            let _ = std::fs::remove_file(&path);
        })
    })
    .await;
    if !matches!(stored, Ok(Ok(_))) {
        tracing::warn!(path = %shown.display(), "the failed dictation could not be kept");
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_session(
    inner: Arc<Inner>,
    session_id: SessionId,
    profile: Arc<crate::config::Profile>,
    req: SessionRequest,
    audio: Arc<Mutex<Vec<f32>>>,
    mut cmd_rx: mpsc::UnboundedReceiver<SessionCmd>,
    wav_path: PathBuf,
    tx: mpsc::Sender<Event>,
    flags: Arc<SessionFlags>,
) {
    let _in_flight = InFlight {
        inner: inner.clone(),
        path: wav_path.clone(),
    };
    // The row that keeps the recording until there is a result.
    let mut held: Option<HistoryId> = None;
    let range = loop {
        match cmd_rx.recv().await {
            Some(SessionCmd::Store) => {
                if held.is_none() {
                    held = hold(&inner, &profile, &req, &audio, &wav_path, &flags).await;
                }
            }
            Some(SessionCmd::Finish(range)) => break range,
            None => {
                // Ended without `finish`. A cancel removes what was kept;
                // a session that was dropped leaves the row and its
                // recording, to be processed again from the history.
                send_done_once(&tx, &flags.done_sent, &session_id, None).await;
                inner.schedule_idle_unload();
                return;
            }
        }
    };
    if held.is_none() {
        held = hold(&inner, &profile, &req, &audio, &wav_path, &flags).await;
    }

    let mut samples = std::mem::take(&mut *audio.lock().unwrap());
    let mut trimmed = false;
    if let Some(range) = range {
        let end = range.end.min(samples.len());
        let start = range.start.min(end);
        if (start, end) != (0, samples.len()) {
            samples = samples[start..end].to_vec();
            trimmed = true;
        }
    }
    let clip = AudioClip {
        samples,
        sample_rate: SAMPLE_RATE,
    };
    if clip.duration_secs() < MIN_UTTERANCE_SECS {
        if let Some(id) = held {
            drop_held(&inner, id, &wav_path).await;
        }
        let _ = tx
            .send(Event::Error {
                step: None,
                code: ErrorCode::NoAudio,
                message: format!("utterance shorter than {MIN_UTTERANCE_SECS} s"),
                recoverable: false,
                hint: None,
            })
            .await;
        send_done_once(&tx, &flags.done_sent, &session_id, None).await;
        inner.schedule_idle_unload();
        return;
    }
    if let (Some(id), true) = (held, trimmed) {
        replace_held(&inner, id, &wav_path, &clip).await;
    }

    tracing::info!(
        session = %session_id,
        profile = %profile.id.0,
        audio_ms = (clip.duration_secs() * 1000.0) as u64,
        "session started"
    );
    let config = inner.config.current();
    let base_prompt = inner
        .config
        .base_prompt()
        .unwrap_or_else(|_| crate::config::base_prompt::DEFAULT_BASE_PROMPT.to_string());
    let ctx = PipelineContext {
        profile: profile.clone(),
        context: req.context.clone(),
        dictionary: Arc::new(config.dictionary.clone()),
        base_prompt,
        general: config.general.clone(),
    };

    let pipeline = match Pipeline::build(&config, &profile, &inner.registry) {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(session = %session_id, code = ?err.code, error = %err.message, "session could not start its steps");
            keep_failed(
                &inner, &config, &profile, &req, &clip, &err, &flags, held, &wav_path,
            )
            .await;
            let _ = tx
                .send(Event::Error {
                    step: None,
                    code: err.code,
                    message: err.message,
                    recoverable: false,
                    hint: err.hint,
                })
                .await;
            send_done_once(&tx, &flags.done_sent, &session_id, None).await;
            inner.schedule_idle_unload();
            return;
        }
    };

    let clip = Arc::new(clip);
    let outcome = match pipeline.run(clip.clone(), &ctx, &tx).await {
        Ok(o) => o,
        Err(err) => {
            tracing::warn!(session = %session_id, code = ?err.code, error = %err.message, "session failed");
            keep_failed(
                &inner, &config, &profile, &req, &clip, &err, &flags, held, &wav_path,
            )
            .await;
            send_done_once(&tx, &flags.done_sent, &session_id, None).await;
            inner.schedule_idle_unload();
            return;
        }
    };

    // From here on the task performs non-abortable spawn_blocking work (a
    // WAV write, a history insert), so tell `cancel` not to abort us
    // anymore: past this point, the `cancelled` checks below are what undo
    // a cancel, not `Session::cancel`'s `task.abort()`.
    tracing::info!(
        session = %session_id,
        asr_ms = outcome.asr_ms,
        llm_ms = ?outcome.llm_ms,
        degraded = outcome.degraded,
        "session processed"
    );
    flags.tail_started.store(true, Ordering::SeqCst);
    if flags.cancelled.load(Ordering::SeqCst) {
        return;
    }

    // A recording kept before the processing is already in place; `cancel`
    // removes it with its row. Otherwise it is written now.
    let audio_path = if held.is_some() {
        Some(wav_path.clone())
    } else if config.general.keep_recordings {
        let bytes = encode_wav_pcm16(&clip);
        let p = wav_path.clone();
        match tokio::task::spawn_blocking(move || std::fs::write(&p, bytes)).await {
            Ok(Ok(())) => Some(wav_path.clone()),
            _ => None,
        }
    } else {
        None
    };
    let written_here = audio_path.clone().filter(|_| held.is_none());

    // Because `tail_started` is now set, `cancel` will not abort this task
    // (see `Session::cancel`), so this check is guaranteed to run rather
    // than racing an abort that could tear the task down first: if
    // cancelled, undo the WAV write above instead of leaving an orphan
    // recording for a cancelled session.
    if flags.cancelled.load(Ordering::SeqCst) {
        if let Some(p) = &written_here {
            let _ = std::fs::remove_file(p);
        }
        return;
    }

    let new = NewUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        raw_text: outcome.raw_text,
        refined_text: outcome.final_text.clone(),
        final_text: outcome.final_text,
        degraded: outcome.degraded,
        audio_path,
        asr_ms: outcome.asr_ms,
        llm_ms: outcome.llm_ms,
        audio_ms: clip_ms(&clip),
        dict_fixes: outcome.dict_fixes,
        snippets: outcome.snippets,
        llm_changed: outcome.llm_changed,
    };
    let history = inner.history.clone();
    let claim = flags.clone();
    #[cfg(test)]
    let tail_delay_ms = inner.tail_delay_ms.load(Ordering::SeqCst);
    // The row is committed only if this task wins `done_sent`: the single
    // point that decides who reports the session's one Done. When `cancel`
    // already claimed it (and sent Done(None)), the row and its statistics
    // are rolled back instead of being written and taken back afterwards.
    // The row that kept the recording goes in the same step.
    let (inserted, claimed) = tokio::task::spawn_blocking(move || {
        // Test-only hook: lets a test land a `cancel()` deterministically
        // while this closure is running, instead of racing real I/O timing.
        #[cfg(test)]
        if tail_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(tail_delay_ms));
        }
        let mut claimed = None;
        let inserted = history.insert_replacing(new, held, || {
            let won = !claim.done_sent.swap(true, Ordering::SeqCst);
            claimed = Some(won);
            won
        });
        (inserted, claimed)
    })
    .await
    .unwrap_or((Ok(None), None));
    if let Err(e) = &inserted {
        tracing::warn!(session = %session_id, error = %e, "history insert failed");
    }
    let history_id = inserted.ok().flatten();

    // The insert can fail before it asks; the claim is then still open.
    let won = claimed.unwrap_or_else(|| !flags.done_sent.swap(true, Ordering::SeqCst));
    if !won {
        if let Some(p) = &written_here {
            let _ = std::fs::remove_file(p);
        }
        tracing::info!(session = %session_id, "cancelled before the result was stored");
        return;
    }

    if let Some(id) = history_id {
        inner
            .sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), id);
    }
    // done_sent was already claimed by the swap above, so send directly
    // rather than through send_done_once (which would just re-check it).
    let _ = tx
        .send(Event::Done {
            session_id: session_id.clone(),
            history_id,
        })
        .await;
    inner.schedule_idle_unload();
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::backup::DeviceMeta;
    use crate::secrets::MemorySecretStore;
    use crate::types::StepName;
    use serde_json::json;
    use tokio_stream::StreamExt;
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Rig {
        _tmp: tempfile::TempDir,
        engine: Engine,
        asr: MockServer,
        llm: MockServer,
    }

    async fn rig(keep_recordings: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let asr = MockServer::start().await;
        let llm = MockServer::start().await;
        let engine = Engine::open(
            DataDir::custom(tmp.path()),
            Arc::new(MemorySecretStore::default()),
        )
        .await
        .unwrap();
        engine
            .config()
            .update(|c| {
                *c = crate::config::Config::http_test_config();
                c.providers[0].base_url = Some(Url::parse(&format!("{}/v1", asr.uri())).unwrap());
                c.providers[1].base_url = Some(Url::parse(&format!("{}/v1", llm.uri())).unwrap());
                c.general.keep_recordings = keep_recordings;
            })
            .unwrap();
        Rig {
            _tmp: tmp,
            engine,
            asr,
            llm,
        }
    }

    #[tokio::test]
    async fn probe_dispatches_to_factory_for_embedded() {
        let tmp = tempfile::tempdir().unwrap();
        let e = Engine::open_with(
            DataDir::custom(tmp.path()),
            EngineOptions {
                secrets: Arc::new(MemorySecretStore::default()),
                factories: vec![Arc::new(crate::providers::registry::tests::FakeFactory)],
                first_run: FirstRunDefaults::default(),
                identity: DeviceOptions::default(),
            },
        )
        .await
        .unwrap();
        let r = e
            .probe_provider(&crate::providers::registry::tests::embedded_cfg())
            .await;
        assert!(r.ok);
    }

    pub(crate) async fn idle_rig() -> (
        Arc<crate::steps::llm::fakes::FakeLlm>,
        tempfile::TempDir,
        Engine,
    ) {
        idle_rig_with(crate::steps::llm::fakes::FakeLlm::ok("ok")).await
    }

    /// `idle_rig` with a caller-supplied shared LLM fake.
    pub(crate) async fn idle_rig_with(
        fake: Arc<crate::steps::llm::fakes::FakeLlm>,
    ) -> (
        Arc<crate::steps::llm::fakes::FakeLlm>,
        tempfile::TempDir,
        Engine,
    ) {
        use crate::config::General;
        use crate::config::Runtime;
        use crate::providers::{AsrProvider, LlmProvider, ProviderError};
        use crate::steps::llm::fakes::FakeLlm;

        // One shared fake LLM so the test can count unloads.
        struct F(Arc<FakeLlm>);
        #[async_trait::async_trait]
        impl ProviderFactory for F {
            fn kind(&self) -> ProviderKind {
                ProviderKind::Embedded
            }
            fn asr(
                &self,
                _: &ProviderConfig,
                _: &General,
            ) -> Result<Arc<dyn AsrProvider>, ProviderError> {
                Ok(crate::steps::asr::fakes::FakeAsr::ok("原文"))
            }
            fn llm(
                &self,
                _: &ProviderConfig,
                _: &General,
            ) -> Result<Arc<dyn LlmProvider>, ProviderError> {
                Ok(self.0.clone())
            }
            async fn probe(&self, _: &ProviderConfig) -> ProbeResult {
                ProbeResult {
                    ok: true,
                    failure: None,
                }
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let e = Engine::open_with(
            DataDir::custom(tmp.path()),
            EngineOptions {
                secrets: Arc::new(MemorySecretStore::default()),
                factories: vec![Arc::new(F(fake.clone()))],
                first_run: FirstRunDefaults::default(),
                identity: DeviceOptions::default(),
            },
        )
        .await
        .unwrap();
        e.config()
            .update(|c| {
                c.general.llm_idle_unload_secs = 2;
                for p in &mut c.providers {
                    p.kind = ProviderKind::Embedded;
                    p.base_url = None;
                    p.runtime = Some(Runtime::Llama);
                    p.model_id = Some("m".into());
                }
            })
            .unwrap();

        (fake, tmp, e)
    }

    pub(crate) async fn settle_and_expect_one_unload(fake: &crate::steps::llm::fakes::FakeLlm) {
        use std::time::Duration;
        // Let the timer be armed before the clock moves.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(Duration::from_secs(3)).await;
        // Let the timer task observe the advanced clock.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn llm_is_unloaded_after_idle_period() {
        let (fake, _tmp, e) = idle_rig().await;
        let s = e.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        drain(&s).await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 0);
        settle_and_expect_one_unload(&fake).await;
    }

    #[tokio::test(start_paused = true)]
    async fn llm_is_unloaded_after_a_cancelled_session() {
        let (fake, _tmp, e) = idle_rig().await;
        // A full session first, so the LLM provider is built and cached.
        let warm = e.start_session(SessionRequest::default());
        warm.push_audio(&one_second());
        warm.finish();
        drain(&warm).await;
        let s = e.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.cancel();
        drain(&s).await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 0);
        settle_and_expect_one_unload(&fake).await;
    }

    #[tokio::test(start_paused = true)]
    async fn llm_is_unloaded_after_a_too_short_session() {
        let (fake, _tmp, e) = idle_rig().await;
        // A full session first, so the LLM provider is built and cached.
        let warm = e.start_session(SessionRequest::default());
        warm.push_audio(&one_second());
        warm.finish();
        drain(&warm).await;
        let s = e.start_session(SessionRequest::default());
        s.push_audio(&[0.0; 100]);
        s.finish();
        drain(&s).await;
        settle_and_expect_one_unload(&fake).await;
    }

    // Paused clock keeps the idle unload timer from adding to the count.
    #[tokio::test(start_paused = true)]
    async fn evict_model_unloads_matching_llm() {
        let (fake, _tmp, e) = idle_rig().await;
        let s = e.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        drain(&s).await;
        e.evict_model("other").await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 0);
        e.evict_model("m").await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 1);
    }

    // The fake factory hands out a fresh provider per build, so a provider
    // that was evicted comes back as a different instance.
    #[tokio::test(start_paused = true)]
    async fn unload_unreferenced_asr_evicts_only_unreferenced_providers() {
        let (_fake, _tmp, e) = idle_rig().await;
        let used = e.config().current().profiles[0].asr_provider.clone();
        let spare = crate::types::ProviderId("spare-asr".into());
        e.config()
            .update(|c| {
                let mut extra = c.provider(&used).unwrap().clone();
                extra.id = spare.clone();
                c.providers.push(extra);
            })
            .unwrap();
        let build = |id: &crate::types::ProviderId| {
            let config = e.config().current();
            e.registry()
                .asr(config.provider(id).unwrap(), &config.general)
                .unwrap()
        };
        let used_before = build(&used);
        let spare_before = build(&spare);
        e.unload_unreferenced_asr().await;
        assert!(Arc::ptr_eq(&used_before, &build(&used)));
        assert!(!Arc::ptr_eq(&spare_before, &build(&spare)));
    }

    #[test]
    fn custom_model_predicate_matches_file_under_custom_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        let mut cfg = crate::providers::registry::tests::embedded_cfg();
        cfg.model_id = None;
        cfg.model_path = Some(dirs.models_dir().join("custom").join("x.gguf"));
        assert!(model_predicate(&dirs, "custom/x.gguf")(&cfg));
        assert!(!model_predicate(&dirs, "custom/y.gguf")(&cfg));
        assert!(!model_predicate(&dirs, "x.gguf")(&cfg));
        cfg.model_path = Some(tmp.path().join("x.gguf"));
        assert!(!model_predicate(&dirs, "custom/x.gguf")(&cfg));
    }

    async fn mock_ok(rig: &Rig, asr_text: &str, llm_text: &str) {
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": asr_text})))
            .mount(&rig.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": llm_text}}]})),
            )
            .mount(&rig.llm)
            .await;
    }

    async fn drain(session: &Session) -> Vec<Event> {
        let mut stream = session.events();
        let mut out = Vec::new();
        while let Some(e) = stream.next().await {
            let is_done = matches!(e, Event::Done { .. });
            out.push(e);
            if is_done {
                break;
            }
        }
        out
    }

    pub(crate) async fn drain_session(session: &Session) -> Vec<Event> {
        drain(session).await
    }

    pub(crate) fn one_second() -> Vec<f32> {
        vec![0.05; SAMPLE_RATE as usize]
    }

    #[tokio::test]
    async fn full_session_writes_history_and_emits_done() {
        let r = rig(false).await;
        mock_ok(&r, "软件 原文", "软件 整理後").await;
        let s = r.engine.start_session(SessionRequest {
            app: AppInfo {
                app_name: Some("TextEdit".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(matches!(
            events.first(),
            Some(Event::SessionStarted {
                protocol_version: 1,
                ..
            })
        ));
        assert!(events.iter().any(
            |e| matches!(e, Event::Refined { text, degraded: false } if text == "軟體 整理後")
        ));
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        assert_eq!(row.raw_text, "软件 原文");
        assert_eq!(row.final_text, "軟體 整理後");
        assert_eq!(row.app_name.as_deref(), Some("TextEdit"));
        assert!(row.audio_path.is_none());
        assert!(row.audio_ms.is_some());

        r.engine.record_final(s.id(), "使用者改過").await.unwrap();
        assert_eq!(
            r.engine.history().get(*id).unwrap().unwrap().final_text,
            "使用者改過"
        );
        assert!(r
            .engine
            .record_final(&SessionId("nope".into()), "x")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn finish_without_audio_emits_error_and_done() {
        let r = rig(false).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text":"x"})))
            .expect(0)
            .mount(&r.asr)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&vec![0.0; 1_000]);
        s.finish();
        let events = drain(&s).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                code: ErrorCode::NoAudio,
                recoverable: false,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancel_while_recording_emits_done_only() {
        let r = rig(false).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.cancel();
        let events = drain(&s).await;
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(matches!(
            events[1],
            Event::Done {
                history_id: None,
                ..
            }
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn asr_unreachable_ends_with_error_then_done() {
        let r = rig(false).await;
        r.engine
            .config()
            .update(|c| {
                c.providers[0].base_url = Some(Url::parse("http://127.0.0.1:9/v1").unwrap())
            })
            .unwrap();
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                code: ErrorCode::ProviderUnreachable,
                recoverable: false,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn keep_recordings_writes_wav_next_to_history() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        let path = row.audio_path.expect("recording path");
        assert!(path.starts_with(r.engine.dirs().recordings_dir()));
        assert!(path.exists());
        assert_eq!(&std::fs::read(&path).unwrap()[0..4], b"RIFF");
    }

    #[tokio::test]
    async fn a_failed_dictation_keeps_its_recording_and_can_be_processed_again() {
        let r = rig(true).await;
        // The speech server answers 400 until the mocks below replace it.
        let down = Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad audio"))
            .mount_as_scoped(&r.asr)
            .await;
        let s = r.engine.start_session(SessionRequest {
            app: AppInfo {
                app_name: Some("Notes".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
        let rows = r.engine.history().list_recent(10).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        let row = &rows[0];
        assert!(
            row.error.as_deref().is_some_and(|e| e.contains("400")),
            "{row:?}"
        );
        assert_eq!(row.final_text, "");
        assert!(row.audio_path.as_ref().is_some_and(|p| p.exists()));
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 0);

        let again = r.engine.reprocess(row.id, None).await;
        assert!(
            matches!(
                again,
                Err(ReprocessError::Failed {
                    code: ErrorCode::ProviderError,
                    ..
                })
            ),
            "{again:?}"
        );
        assert_eq!(row.error_code.as_deref(), Some("provider_error"));

        drop(down);
        mock_ok(&r, "原文", "整理後").await;
        let done = r.engine.reprocess(row.id, None).await.unwrap();
        assert_eq!(done.redo.final_text, "整理後");
        assert_eq!(done.redo.raw_text, "原文");
        assert_eq!(done.redo.profile_id, row.profile_id);
        assert!(done.llm_label.is_some());
        // Nothing is stored until the result is applied.
        assert!(r
            .engine
            .history()
            .get(row.id)
            .unwrap()
            .unwrap()
            .error
            .is_some());
        assert!(r.engine.history().apply_redo(row.id, &done.redo).unwrap());
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 1);
    }

    #[tokio::test]
    async fn a_failed_dictation_leaves_nothing_when_recordings_are_not_kept() {
        let r = rig(false).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&r.asr)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        drain(&s).await;
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
        let id = HistoryId(1);
        assert!(matches!(
            r.engine.reprocess(id, None).await,
            Err(ReprocessError::NoRow)
        ));
    }

    #[tokio::test]
    async fn profile_override_is_reported_in_session_started() {
        let r = rig(false).await;
        r.engine
            .config()
            .update(|c| {
                let mut p = c.profiles[0].clone();
                p.id = ProfileId("slack".into());
                p.is_default = false;
                c.profiles.push(p);
            })
            .unwrap();
        let s = r.engine.start_session(SessionRequest {
            profile_override: Some(ProfileId("slack".into())),
            ..Default::default()
        });
        assert_eq!(s.profile_id().0, "slack");
        s.cancel();
    }

    #[tokio::test]
    async fn cancel_while_pipeline_runs_emits_one_done_and_records_nothing() {
        let r = rig(true).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "原文"})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": "整理後"}}]}))
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&r.llm)
            .await;

        let s = r.engine.start_session(SessionRequest::default());
        // Take the stream now: the channel only closes once every Sender is
        // dropped, which includes the one `Session` itself holds, so this
        // stream is dropped along with `s` below to let it end.
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        s.cancel();
        drop(s);

        let mut events = Vec::new();
        while let Some(e) = stream.next().await {
            events.push(e);
        }

        let dones: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::Done { .. }))
            .collect();
        assert_eq!(dones.len(), 1, "{events:?}");
        assert!(matches!(
            dones[0],
            Event::Done {
                history_id: None,
                ..
            }
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 0);
        let recordings = r.engine.dirs().recordings_dir();
        let left_a_file = std::fs::read_dir(&recordings)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        assert!(!left_a_file, "cancel must not leave a recording behind");
    }

    #[tokio::test]
    async fn cancel_during_tail_records_nothing_and_emits_one_done() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        // Slows down the history insert so a cancel can be landed
        // deterministically while it is running, rather than racing real
        // I/O timing the way `cancel_while_pipeline_runs_...` does for the
        // (already abortable) network-await window.
        r.engine.inner.tail_delay_ms.store(300, Ordering::SeqCst);

        let s = r.engine.start_session(SessionRequest::default());
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();

        // Wait for Refined: the pipeline has finished and the task is now
        // in the (non-abortable) tail, about to write the WAV and insert
        // the history row.
        while let Some(e) = stream.next().await {
            if matches!(e, Event::Refined { .. }) {
                break;
            }
        }
        // Land the cancel inside the 300 ms insert delay above, rather than
        // racing it, so this deterministically exercises the claim-loser
        // delete path in `run_session` (the `done_sent.swap` branch that
        // deletes an already-inserted row).
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        s.cancel();
        drop(s);

        let mut events = Vec::new();
        while let Some(e) = stream.next().await {
            events.push(e);
        }

        let dones: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::Done { .. }))
            .collect();
        assert_eq!(dones.len(), 1, "{events:?}");
        assert!(matches!(
            dones[0],
            Event::Done {
                history_id: None,
                ..
            }
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 0);
        let recordings = r.engine.dirs().recordings_dir();
        let left_a_file = std::fs::read_dir(&recordings)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        assert!(!left_a_file, "cancel must not leave a recording behind");
    }

    #[tokio::test]
    async fn cancel_after_completion_is_a_noop() {
        let r = rig(false).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();

        let mut id = None;
        while let Some(e) = stream.next().await {
            if let Event::Done { history_id, .. } = e {
                id = history_id;
                break;
            }
        }
        let id = id.expect("session should complete with a history id");

        s.cancel();
        s.cancel();

        let result =
            tokio::time::timeout(std::time::Duration::from_millis(100), stream.next()).await;
        assert!(
            matches!(result, Err(_) | Ok(None)),
            "cancel after completion must not emit another event: {result:?}"
        );
        assert!(r.engine.history().get(id).unwrap().is_some());
    }

    #[tokio::test]
    async fn empty_transcript_ends_with_error_and_no_history() {
        let r = rig(false).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": ""})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": "x"}}]})),
            )
            .expect(0)
            .mount(&r.llm)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                step: Some(StepName::Asr),
                code: ErrorCode::NoSpeech,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_llm_key_degrades_to_asr_text() {
        let r = rig(false).await;
        r.engine
            .config()
            .update(|c| c.providers[1].api_key_ref = Some("absent".into()))
            .unwrap();
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "原文"})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": "x"}}]})),
            )
            .expect(0)
            .mount(&r.llm)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Refined { text, degraded: true } if text == "原文")));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: Some(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn llm_5xx_degrades_with_history_row() {
        let r = rig(false).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "原文"})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&r.llm)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Refined { text, degraded: true } if text == "原文")));
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        assert!(row.degraded);
    }

    /// Waits until the history holds what `want` asks for.
    async fn rows_when(
        r: &Rig,
        want: impl Fn(&[crate::history::Utterance]) -> bool,
    ) -> Vec<crate::history::Utterance> {
        for _ in 0..200 {
            let rows = r.engine.history().list_recent(10).unwrap();
            if want(&rows) {
                return rows;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("history: {:?}", r.engine.history().list_recent(10).unwrap());
    }

    #[tokio::test]
    async fn the_recording_is_kept_before_it_is_processed() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.store();
        // Nothing was asked of the engines yet; the recording is on disk
        // with a row that says the dictation did not finish.
        let rows = rows_when(&r, |rows| rows.len() == 1).await;
        let path = rows[0].audio_path.clone().unwrap();
        assert_eq!(rows[0].error_code.as_deref(), Some("interrupted"));
        assert_eq!(rows[0].audio_ms, Some(1000));
        assert!(path.starts_with(r.engine.recordings_dir()) && path.is_file());
        assert!(r.engine.in_flight(&path));

        s.finish();
        let events = drain(&s).await;
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        // The result took that row's place and kept the file.
        let rows = r.engine.history().list_recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, *id);
        assert_eq!(rows[0].error, None);
        assert_eq!(rows[0].audio_path.as_deref(), Some(path.as_path()));
        assert!(path.is_file());
        for _ in 0..100 {
            if !r.engine.in_flight(&path) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!r.engine.in_flight(&path));
    }

    #[tokio::test]
    async fn a_session_dropped_after_store_leaves_its_recording_to_retry() {
        let r = rig(true).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.store();
        drop(s);
        let rows = rows_when(&r, |rows| rows.len() == 1).await;
        assert_eq!(rows[0].error_code.as_deref(), Some("interrupted"));
        let path = rows[0].audio_path.clone().unwrap();
        assert!(path.is_file());
        for _ in 0..100 {
            if !r.engine.in_flight(&path) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!r.engine.in_flight(&path));
        // It can be processed again like any failed row.
        mock_ok(&r, "原文", "整理後").await;
        let again = r.engine.reprocess(rows[0].id, None).await.unwrap();
        assert_eq!(again.redo.final_text, "整理後");
    }

    #[tokio::test]
    async fn a_cancel_after_store_leaves_nothing() {
        let r = rig(true).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.store();
        let rows = rows_when(&r, |rows| rows.len() == 1).await;
        let path = rows[0].audio_path.clone().unwrap();
        s.cancel();
        rows_when(&r, |rows| rows.is_empty()).await;
        for _ in 0..100 {
            if !path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn the_kept_recording_becomes_the_part_that_was_processed() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        let mut audio = one_second();
        audio.extend(one_second());
        s.push_audio(&audio);
        s.store();
        let half = SAMPLE_RATE as usize / 2;
        s.finish_range(half..half + SAMPLE_RATE as usize);
        let events = drain(&s).await;
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        assert_eq!(row.audio_ms, Some(1000));
        let bytes = std::fs::read(row.audio_path.unwrap()).unwrap();
        let clip = crate::audio::decode_wav_pcm16(&bytes).unwrap();
        assert_eq!(clip.samples.len(), SAMPLE_RATE as usize);
        // No leftover of the replacement.
        let names: Vec<_> = std::fs::read_dir(r.engine.recordings_dir())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[tokio::test]
    async fn a_failure_names_its_reason_in_the_row_that_kept_the_recording() {
        let r = rig(true).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad audio"))
            .mount(&r.asr)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.store();
        s.finish();
        drain(&s).await;
        let rows = r.engine.history().list_recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].error_code.as_deref(), Some("provider_error"));
        assert!(rows[0].audio_path.as_ref().unwrap().is_file());
    }

    #[tokio::test]
    async fn nothing_is_written_when_recordings_are_not_kept() {
        let r = rig(false).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.store();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
        assert_eq!(
            std::fs::read_dir(r.engine.recordings_dir())
                .unwrap()
                .count(),
            0
        );
        s.cancel();
    }

    #[tokio::test]
    async fn recordings_follow_the_folder_the_user_chose() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let default = r.engine.recordings_dir();
        let chosen = r._tmp.path().join("elsewhere");

        let report = r
            .engine
            .set_recordings_dir(Some(chosen.clone()))
            .await
            .unwrap();
        assert_eq!(report.moved, 1);
        assert_eq!(r.engine.recordings_dir(), chosen);
        let row = r.engine.history().get(*id).unwrap().unwrap();
        let moved = row.audio_path.unwrap();
        assert!(moved.starts_with(&chosen) && moved.is_file());
        // A new dictation lands there too, and can be processed again.
        r.engine.reprocess(*id, None).await.unwrap();

        // Back to the default folder.
        r.engine.set_recordings_dir(None).await.unwrap();
        assert_eq!(r.engine.recordings_dir(), default);
        assert_eq!(r.engine.config().current().general.recordings_dir, None);
        let row = r.engine.history().get(*id).unwrap().unwrap();
        assert!(row.audio_path.unwrap().starts_with(&default));

        assert!(matches!(
            r.engine.set_recordings_dir(Some("relative".into())).await,
            Err(EngineError::Folder(_))
        ));
    }

    #[tokio::test]
    async fn recordings_of_an_earlier_build_move_out_of_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDir::custom(tmp.path());
        let legacy = dirs.legacy_recordings_dir();
        let old = legacy.join("a.wav");
        {
            let e = Engine::open(dirs.clone(), Arc::new(MemorySecretStore::default()))
                .await
                .unwrap();
            std::fs::create_dir_all(&legacy).unwrap();
            std::fs::write(&old, b"RIFF").unwrap();
            let mut row = crate::history::tests_support::utterance();
            row.audio_path = Some(old.clone());
            e.history().insert(row).unwrap();
        }
        let e = Engine::open(dirs.clone(), Arc::new(MemorySecretStore::default()))
            .await
            .unwrap();
        let rows = e.history().list_recent(10).unwrap();
        let new = dirs.recordings_dir().join("a.wav");
        assert_eq!(rows[0].audio_path.as_deref(), Some(new.as_path()));
        assert!(new.is_file() && !old.exists() && !legacy.exists());
    }

    #[tokio::test]
    async fn renaming_this_device_is_kept_across_a_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let open = || {
            Engine::open(
                DataDir::custom(tmp.path()),
                Arc::new(MemorySecretStore::default()),
            )
        };
        let e = open().await.unwrap();
        let id = e.device().id;
        e.rename_device(&id, "Desk").await.unwrap();
        assert_eq!(e.device().name, "Desk");
        assert_eq!(e.devices().unwrap()[0].name, "Desk");
        drop(e);
        let e = open().await.unwrap();
        assert_eq!(
            (e.device().id, e.device().name),
            (id.clone(), "Desk".to_string())
        );
        assert_eq!(e.devices().unwrap()[0].name, "Desk");
        assert!(e.remove_device(&id).await.is_err());
        assert!(e.rename_device(&id, "  ").await.is_err());
    }

    fn row(ts: i64, text: &str) -> NewUtterance {
        NewUtterance {
            ts,
            profile_id: ProfileId("default".into()),
            app_name: Some("Notes".into()),
            raw_text: text.into(),
            refined_text: text.into(),
            final_text: text.into(),
            degraded: false,
            audio_path: None,
            asr_ms: 10,
            llm_ms: None,
            audio_ms: None,
            dict_fixes: 0,
            snippets: 0,
            llm_changed: false,
        }
    }

    fn text_backup() -> Categories {
        Categories {
            history: true,
            stats: true,
            recordings: false,
            settings: true,
        }
    }

    async fn plain_engine(root: &Path) -> Engine {
        Engine::open(
            DataDir::custom(root),
            Arc::new(MemorySecretStore::default()),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_new_database_gets_a_new_device_id() {
        let tmp = tempfile::tempdir().unwrap();
        let e = plain_engine(tmp.path()).await;
        let first = e.device();
        let db = e.dirs().history_path();
        drop(e);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
        }
        let e = plain_engine(tmp.path()).await;
        assert_ne!(e.device().id, first.id);
        assert_eq!(e.device().name, first.name);
        drop(e);
        // The database is there now: the identity stays.
        let again = plain_engine(tmp.path()).await;
        assert_eq!(again.devices().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_backup_file_round_trips_through_another_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let a = plain_engine(&tmp.path().join("a")).await;
        a.history().insert(row(1_790_812_800, "one")).unwrap();
        a.history().insert(row(1_790_812_900, "two")).unwrap();
        let file = tmp.path().join("out.arcvox");
        let wrote = a
            .backup_export_file(&file, text_backup(), Some(b"{}".to_vec()))
            .await
            .unwrap();
        assert_eq!(wrote.rows, 2);
        assert!(file.is_file());

        let b = plain_engine(&tmp.path().join("b")).await;
        b.history().insert(row(1_790_813_000, "mine")).unwrap();
        let source = BackupSource::File(file);
        let info = b.backup_inspect(&source).await.unwrap();
        assert_eq!(info.devices.len(), 1);
        assert_eq!(
            (
                info.devices[0].rows,
                info.devices[0].this,
                info.devices[0].has_settings
            ),
            (2, false, true)
        );
        let report = b
            .backup_import(&source, vec![a.device().id], text_backup())
            .await
            .unwrap();
        assert_eq!((report.devices, report.added), (1, 2));
        assert_eq!(b.devices().unwrap().len(), 2);
        assert_eq!(b.history().list(10, None, false).unwrap().len(), 3);
        // Nothing of the staging is left in the cache directories.
        for e in [&a, &b] {
            let left: Vec<_> = std::fs::read_dir(&e.dirs().cache_dir)
                .unwrap()
                .flatten()
                .map(|d| d.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with(".tmp"))
                .collect();
            assert!(left.is_empty(), "{left:?}");
        }
    }

    #[tokio::test]
    async fn the_folder_write_only_has_this_device() {
        let tmp = tempfile::tempdir().unwrap();
        let a = plain_engine(&tmp.path().join("a")).await;
        a.history().insert(row(1_790_812_800, "one")).unwrap();
        let folder = tmp.path().join("sync");
        std::fs::create_dir_all(&folder).unwrap();
        a.backup_write_folder(&folder, text_backup(), None)
            .await
            .unwrap();

        let b = plain_engine(&tmp.path().join("b")).await;
        b.history().insert(row(1_790_812_800, "other")).unwrap();
        b.backup_import(
            &BackupSource::Folder(folder.clone()),
            vec![a.device().id],
            text_backup(),
        )
        .await
        .unwrap();
        b.backup_write_folder(&folder, text_backup(), None)
            .await
            .unwrap();
        let info = b
            .backup_inspect(&BackupSource::Folder(folder.clone()))
            .await
            .unwrap();
        assert_eq!(info.devices.len(), 2);
        assert!(info.devices.iter().all(|d| d.rows == 1));
        // The other device's directory was written by that device alone.
        let a_dir = folder.join("devices").join(a.device().id);
        let written: DeviceMeta =
            serde_json::from_slice(&std::fs::read(a_dir.join("device.json")).unwrap()).unwrap();
        assert_eq!(written.name, a.device().name);
    }

    #[tokio::test]
    async fn a_backup_folder_that_is_gone_is_not_made_again() {
        let tmp = tempfile::tempdir().unwrap();
        let a = plain_engine(&tmp.path().join("a")).await;
        a.history().insert(row(1_790_812_800, "one")).unwrap();
        let folder = tmp.path().join("unmounted").join("sync");
        let got = a.backup_write_folder(&folder, text_backup(), None).await;
        assert!(matches!(
            got,
            Err(EngineError::Backup(BackupError::Io { .. }))
        ));
        assert!(!tmp.path().join("unmounted").exists());
    }

    #[tokio::test]
    async fn import_is_refused_while_a_dictation_is_in_flight() {
        let tmp = tempfile::tempdir().unwrap();
        let a = plain_engine(&tmp.path().join("a")).await;
        a.history().insert(row(1_790_812_800, "one")).unwrap();
        let folder = tmp.path().join("sync");
        std::fs::create_dir_all(&folder).unwrap();
        a.backup_write_folder(&folder, text_backup(), None)
            .await
            .unwrap();
        let b = plain_engine(&tmp.path().join("b")).await;
        b.inner
            .in_flight
            .lock()
            .unwrap()
            .insert(PathBuf::from("/r/x.wav"));
        assert!(b.busy());
        let got = b
            .backup_import(
                &BackupSource::Folder(folder),
                vec![a.device().id],
                text_backup(),
            )
            .await;
        assert!(
            matches!(got, Err(EngineError::Busy)),
            "{:?}",
            got.map(|_| ())
        );
        assert_eq!(b.devices().unwrap().len(), 1);
    }
}
