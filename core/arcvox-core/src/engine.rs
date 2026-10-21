//! The crate's entry point. Owns config, history, and providers; opens sessions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::audio::encode_wav_pcm16;
use crate::backup::{self, BackupError, BackupInfo, Categories, ImportReport, Scope, WriteReport};
use crate::config::{
    ConfigStore, FirstRunDefaults, Profile, ProviderConfig, ProviderKind, ResolvedEngines,
    StoreError,
};
use crate::device::Device;
use crate::history::{
    Delivery, DeviceRef, DeviceRow, HistoryError, HistoryStore, MoveReport, NewUtterance,
    StageKind, StageRecord,
};
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
use crate::types::{
    AppInfo, AudioClip, Context, ErrorCode, HistoryId, ProfileId, ProviderId, SessionId,
};
use crate::SAMPLE_RATE;

/// Shorter utterances are rejected before any provider call.
pub const MIN_UTTERANCE_SECS: f32 = 0.2;
/// The longest recording a session keeps; audio past it is dropped. Clients
/// end a recording at their own, shorter limit; this one bounds the memory
/// a session can take whatever the client does.
pub const MAX_UTTERANCE_SECS: u32 = 30 * 60;
const MAX_UTTERANCE_SAMPLES: usize = MAX_UTTERANCE_SECS as usize * SAMPLE_RATE as usize;
const EVENT_BUFFER: usize = 256;
/// Finished sessions whose history row is still remembered for
/// `record_final`. Dictations happen one at a time, so once this many have
/// gone without a final text, every one of them is stale.
const SESSIONS_KEPT: usize = 16;
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
    /// A blocking task (history, backup, a file move) stopped before it
    /// produced a result, usually because it panicked.
    #[error("a background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
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

/// The result of `Engine::reprocess`, recorded with the row. `chosen`
/// says the row took it at once (it had failed before).
#[derive(Debug, Clone, PartialEq)]
pub struct Reprocessed {
    pub attempt: crate::history::Attempt,
    pub chosen: bool,
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
            .await??,
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
                general.stage_retention_days,
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
                        general.stage_retention_days,
                        &recordings,
                    )
                })
                .await;
                match pruned {
                    Ok(Ok(report)) => tracing::info!(
                        rows = report.rows_deleted,
                        recordings = report.recordings_deleted,
                        stages = report.stages_deleted,
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
            let engines = inner.resolve_engines(&config, profile);
            let Some(cfg) = config.provider(&engines.asr.id) else {
                return;
            };
            if let Ok(p) = inner.registry.asr(cfg, &config.general) {
                let _ = p.load().await;
            }
        });
    }

    /// Whether an engine can run now, by the factory of its kind.
    pub fn can_run(&self, cfg: &ProviderConfig) -> bool {
        self.inner.registry.ready(cfg)
    }

    /// The engines a profile runs with now: its own when they can run,
    /// else the default pair. See `Config::resolve_engines`.
    pub fn resolve_engines(&self, profile: &Profile) -> ResolvedEngines {
        let config = self.inner.config.current();
        self.inner.resolve_engines(&config, profile)
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
        Ok(tokio::task::spawn_blocking(move || history.remove_device(&id, &recordings)).await??)
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
        recreate: bool,
    ) -> Result<WriteReport, EngineError> {
        // A folder that is gone (a volume that is not mounted, a folder that
        // was removed) is reported, not made again somewhere else, unless
        // the caller asks for it.
        let gone = !dir.is_dir();
        if gone && !recreate {
            return Err(BackupError::Io {
                path: dir.to_path_buf(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the folder does not exist",
                ),
            }
            .into());
        }
        let mut report = self
            .write_backup(dir.to_path_buf(), cats, Scope::ThisDevice, settings)
            .await?;
        report.recreated = gone;
        Ok(report)
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
        .await??;
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
        .await??)
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
        .await??)
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
        .await??)
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
        .await??;
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
        if config.general.llm_preload_on_record {
            self.inner.preload_llm(&config, &profile);
        }
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
            max_samples: MAX_UTTERANCE_SAMPLES,
            cmd_tx: Mutex::new(Some(cmd_tx)),
            events_tx,
            events_rx: Mutex::new(Some(events_rx)),
            task: Mutex::new(Some(task)),
            flags,
            on_cancel_done: {
                let inner = self.inner.clone();
                Arc::new(move |keep: bool| {
                    inner.schedule_idle_unload();
                    if !keep {
                        inner.discard_held(wav_path.clone());
                    }
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
        let (row, origin, clip) = tokio::task::spawn_blocking(move || {
            let row = history.get(id)?.ok_or(ReprocessError::NoRow)?;
            // The profile the dictation was made with, when the row shows
            // another result's by now.
            let origin = history
                .attempts(id)?
                .into_iter()
                .find(|a| a.origin)
                .map(|a| a.profile_id);
            let path = history
                .recording_path(id, &recordings)?
                .ok_or(ReprocessError::NoRecording)?;
            let bytes = std::fs::read(&path).map_err(|_| ReprocessError::NoRecording)?;
            let clip =
                crate::audio::decode_wav_pcm16(&bytes).map_err(|_| ReprocessError::NoRecording)?;
            Ok::<_, ReprocessError>((row, origin, clip))
        })
        .await
        .map_err(|e| ReprocessError::Failed {
            code: ErrorCode::Internal,
            text: e.to_string(),
        })??;

        let config = self.inner.config.current();
        let wanted = profile.unwrap_or_else(|| origin.unwrap_or_else(|| row.profile_id.clone()));
        let profile =
            Arc::new(resolve(&config.profiles, &AppInfo::default(), Some(&wanted)).clone());
        let engines = self.inner.resolve_engines(&config, &profile);
        let label = |id: &crate::types::ProviderId| {
            config
                .provider(id)
                .map(|p| p.label.clone())
                .unwrap_or_else(|| id.0.clone())
        };
        let asr_label = label(&engines.asr.id);
        let llm_label = engines.llm.as_ref().map(|c| label(&c.id));

        self.mark_busy();
        let _idle = RearmIdle(self);
        let fail = |e: &crate::steps::StepError| ReprocessError::Failed {
            code: e.code,
            text: failure_text(e),
        };
        let pipeline = Pipeline::build(&config, &profile, &engines, &self.inner.registry)
            .map_err(|e| fail(&e))?;
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
                recognised: Default::default(),
                asr_sample: None,
                general: config.general.clone(),
            };
        // Nobody listens to the step events of a rerun.
        let (tx, mut rx) = mpsc::channel(EVENT_BUFFER);
        let drain = self
            .inner
            .runtime
            .spawn(async move { while rx.recv().await.is_some() {} });
        let clip_duration_ms = clip_ms(&clip).unwrap_or(0);
        let outcome = pipeline.run(Arc::new(clip), &ctx, &tx).await;
        drop(tx);
        let _ = drain.await;
        // Recorded with the row either way; a failed run is a result too.
        let recorded = match &outcome {
            Ok(out) => crate::history::Outcome::Done {
                redo: crate::history::Redo {
                    profile_id: profile.id.clone(),
                    raw_text: out.raw_text.clone(),
                    final_text: out.final_text.clone(),
                    degraded: out.degraded,
                    asr_ms: out.asr_ms,
                    llm_ms: out.llm_ms,
                    dict_fixes: out.dict_fixes,
                    snippets: out.snippets,
                    llm_changed: out.llm_changed,
                    dict_hits: out.dict_hits.clone(),
                    // A rerun starts from the kept recording: nothing to
                    // trim, nothing inserted.
                    stages: std::iter::once(StageRecord {
                        position: StageKind::Capture.position(false),
                        kind: StageKind::Capture,
                        text: None,
                        ms: None,
                        detail: serde_json::json!({ "duration_ms": clip_duration_ms }),
                    })
                    .chain(out.stages.iter().cloned())
                    .chain(std::iter::once(StageRecord {
                        position: StageKind::Record.position(false),
                        kind: StageKind::Record,
                        text: None,
                        ms: None,
                        detail: serde_json::json!({ "history": true, "recording": true }),
                    }))
                    .collect(),
                },
                asr_label,
                llm_label,
            },
            Err(e) => crate::history::Outcome::Failed {
                profile_id: profile.id.clone(),
                code: e.code,
                text: failure_text(e),
            },
        };
        let history = self.inner.history.clone();
        let mut recorded = recorded;
        let general = self.inner.config.current().general.clone();
        if let crate::history::Outcome::Done { redo, .. } = &mut recorded {
            if !general.keep_stages {
                redo.stages.clear();
            }
            // The counts of use go with the statistics.
            if !general.keep_stats {
                redo.dict_hits.clear();
            }
        }
        let stored =
            tokio::task::spawn_blocking(move || history.record_attempt(id, &recorded, false))
                .await
                .map_err(|e| ReprocessError::Failed {
                    code: ErrorCode::Internal,
                    text: e.to_string(),
                })??;
        let outcome = outcome.map_err(|e| fail(&e))?;
        drop(outcome);
        let attempt = stored.ok_or(ReprocessError::NoRow)?;
        Ok(Reprocessed {
            chosen: attempt.chosen,
            attempt,
        })
    }

    /// Records what the user actually inserted for a finished session, and
    /// how it was delivered when the client says.
    /// Records the text the dictation ended with and returns the row it
    /// went to, for a correction that may follow.
    pub async fn record_final(
        &self,
        session_id: &SessionId,
        text: &str,
        delivery: Option<Delivery>,
    ) -> Result<HistoryId, EngineError> {
        let history_id = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .remove(session_id)
            .ok_or_else(|| EngineError::UnknownSession(session_id.clone()))?;
        let history = self.inner.history.clone();
        let text = text.to_string();
        let delivery = delivery.filter(|_| self.inner.config.current().general.keep_stages);
        tokio::task::spawn_blocking(move || match delivery {
            Some(d) => history.set_final_delivered(history_id, &text, &d),
            None => history.set_final(history_id, &text),
        })
        .await??;
        Ok(history_id)
    }

    /// The text the user kept, found in the application after the
    /// dictation: it replaces the row's final text.
    pub async fn correct_final(
        &self,
        id: HistoryId,
        text: &str,
        source: crate::history::CorrectionSource,
    ) -> Result<(), EngineError> {
        let history = self.inner.history.clone();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || history.set_final_from(id, &text, Some(source)))
            .await??;
        Ok(())
    }

    /// The newest finished dictation made with a profile, with what each
    /// of its stages did.
    pub async fn last_run(
        &self,
        profile: &ProfileId,
    ) -> Result<Option<(crate::history::Utterance, Vec<StageRecord>)>, EngineError> {
        let history = self.inner.history.clone();
        let profile = profile.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let Some(row) = history.last_for_profile(&profile)? else {
                return Ok::<_, HistoryError>(None);
            };
            let stages = history.stages(row.id)?;
            Ok(Some((row, stages)))
        })
        .await??)
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

    /// Drops and unloads every cached speech provider no profile runs with,
    /// so a model loaded only to be timed does not stay in memory.
    pub async fn unload_unreferenced_asr(&self) {
        let config = self.inner.config.current();
        let in_use: std::collections::HashSet<ProviderId> = config
            .profiles
            .iter()
            .map(|p| self.inner.resolve_engines(&config, p).asr.id)
            .collect();
        let evicted = self
            .inner
            .registry
            .evict_asr_where(|cfg| !in_use.contains(&cfg.id));
        for p in evicted {
            p.unload().await;
        }
    }

    /// Drops every cached provider that loads the model `id` and unloads it,
    /// so a deleted file frees its memory now and the next session reports
    /// the missing model instead of using the copy still in memory.
    pub async fn evict_model(&self, models_dir: &Path, id: &str) {
        let (asr, llm) = self
            .inner
            .registry
            .evict_where(model_predicate(models_dir, id));
        for p in asr {
            p.unload().await;
        }
        for p in llm {
            p.unload().await;
        }
    }

    /// Drops and unloads every cached embedded provider, so the model files
    /// can be moved.
    pub async fn evict_embedded(&self) {
        let (asr, llm) = self
            .inner
            .registry
            .evict_where(|cfg| cfg.kind == ProviderKind::Embedded);
        for p in asr {
            p.unload().await;
        }
        for p in llm {
            p.unload().await;
        }
    }

    /// Whether a dictation is being processed.
    pub fn is_busy(&self) -> bool {
        !self.inner.in_flight.lock().unwrap().is_empty()
    }
}

/// Which embedded provider configs load the model `id` names: a catalogue id,
/// or `custom/<file>` for an imported file under `models_dir`.
pub(crate) fn model_predicate(models_dir: &Path, id: &str) -> impl Fn(&ProviderConfig) -> bool {
    let custom_dir = models_dir.join("custom");
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
    /// Loads the profile's language model in the background while the
    /// recording goes on, so the rewrite does not wait for it afterwards.
    /// Failures are left to the session, which reports them when it needs
    /// the provider.
    fn preload_llm(
        self: &Arc<Self>,
        config: &crate::config::Config,
        profile: &crate::config::Profile,
    ) {
        let engines = self.resolve_engines(config, profile);
        let Some(cfg) = engines.llm.as_ref().and_then(|c| config.provider(&c.id)) else {
            return;
        };
        let Ok(p) = self.registry.llm(cfg, &config.general) else {
            return;
        };
        self.runtime.spawn(async move {
            let _ = p.load().await;
        });
    }

    /// `Config::resolve_engines` with this engine's factories saying which
    /// engines can run.
    fn resolve_engines(
        &self,
        config: &crate::config::Config,
        profile: &Profile,
    ) -> ResolvedEngines {
        config.resolve_engines(profile, |cfg| self.registry.ready(cfg))
    }

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
/// Says that the result is there but could not be kept; the client shows
/// it after the text has been put in place.
async fn storage_failed(tx: &mpsc::Sender<Event>, message: String) {
    let _ = tx
        .send(Event::Error {
            step: None,
            code: ErrorCode::StorageFailed,
            message,
            recoverable: true,
            hint: None,
        })
        .await;
}

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

/// The row that keeps a recording while it is being made, and the file it
/// is written to as it comes in. Only when recordings are kept; None when
/// the row or the file could not be made, in which case `hold` writes the
/// whole recording at the end as before.
async fn start_journal(
    inner: &Arc<Inner>,
    profile: &crate::config::Profile,
    req: &SessionRequest,
    path: &Path,
) -> Option<(HistoryId, crate::journal::Journal)> {
    if !inner.config.current().general.keeps_recordings() {
        return None;
    }
    let row = crate::history::FailedUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        audio_path: Some(path.to_path_buf()),
        audio_ms: None,
        code: ErrorCode::Interrupted,
        error: INTERRUPTED.into(),
        raw_text: String::new(),
        final_text: String::new(),
    };
    inner.in_flight.lock().unwrap().insert(path.to_path_buf());
    let history = inner.history.clone();
    let p = path.to_path_buf();
    let started = tokio::task::spawn_blocking(move || {
        let journal = crate::journal::Journal::open(p.clone(), history.clone())?;
        let id = match history.insert_failed(row) {
            Ok(id) => id,
            Err(e) => {
                drop(journal);
                let _ = std::fs::remove_file(&p);
                return Err(e);
            }
        };
        journal.set_row(id);
        Ok::<_, HistoryError>((id, journal))
    })
    .await;
    match started {
        Ok(Ok(found)) => Some(found),
        Ok(Err(e)) => {
            tracing::warn!(path = %path.display(), error = %e, "the recording's file could not be started");
            None
        }
        Err(_) => None,
    }
}

/// Closes the recording's file. When the file could not be made whole, the
/// row and the file go, and `hold` writes the whole recording afterwards.
async fn settle_journal(
    inner: &Arc<Inner>,
    journal: &mut Option<crate::journal::Journal>,
    held: &mut Option<HistoryId>,
    path: &Path,
) {
    let Some(j) = journal.take() else {
        return;
    };
    if !j.close().await {
        tracing::warn!(path = %path.display(), "the recording's file was not whole; written again");
        if let Some(id) = held.take() {
            drop_held(inner, id, path).await;
        }
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
    if !inner.config.current().general.keeps_recordings() {
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
        audio_path: Some(path.to_path_buf()),
        audio_ms: clip_ms(&clip),
        code: ErrorCode::Interrupted,
        error: INTERRUPTED.into(),
        raw_text: String::new(),
        final_text: String::new(),
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
        // A cancel that came while this ran found nothing to remove; a
        // kept cancel takes the row over.
        if flags.cancelled.load(Ordering::SeqCst) && !flags.keep.load(Ordering::SeqCst) {
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

/// Keeps a dictation that failed as a history row that says why, with its
/// recording when recordings are kept, so it can be processed again.
/// Silence and a discarded cancel leave nothing. `held` is the row that
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
        || (flags.cancelled.load(Ordering::SeqCst) && !flags.keep.load(Ordering::SeqCst))
        || !config.general.keep_history;
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
    if nothing_to_keep {
        return;
    }
    let failed = crate::history::FailedUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        audio_path: config
            .general
            .keeps_recordings()
            .then(|| path.to_path_buf()),
        audio_ms: clip_ms(clip),
        code: err.code,
        error: failure_text(err),
        raw_text: String::new(),
        final_text: String::new(),
    };
    if !insert_with_recording(inner, failed, clip, false).await {
        tracing::warn!(path = %path.display(), "the failed dictation could not be kept");
    }
}

/// Writes a row without a result, and its recording first when the row
/// names one and `written` says the file is not there yet. True when the
/// row is in; a recording that could not be written leaves a row without.
async fn insert_with_recording(
    inner: &Arc<Inner>,
    mut row: crate::history::FailedUtterance,
    clip: &AudioClip,
    written: bool,
) -> bool {
    let bytes = (row.audio_path.is_some() && !written).then(|| encode_wav_pcm16(clip));
    let history = inner.history.clone();
    let stored = tokio::task::spawn_blocking(move || {
        if let (Some(path), Some(bytes)) = (row.audio_path.clone(), bytes) {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!(path = %path.display(), error = %e, "the recording could not be written");
                row.audio_path = None;
            }
        }
        history.insert_failed(row)
    })
    .await;
    matches!(stored, Ok(Ok(_)))
}

/// The row of a dictation that was cancelled and kept: the row that held
/// the recording takes the cancel over, else a new one is written, with
/// the recording when recordings are kept (`written` says the file is
/// there already). A clip too short to hold speech leaves nothing.
#[allow(clippy::too_many_arguments)]
async fn store_cancelled(
    inner: &Arc<Inner>,
    profile: &crate::config::Profile,
    req: &SessionRequest,
    clip: &AudioClip,
    held: Option<HistoryId>,
    path: &Path,
    texts: (&str, &str),
    trimmed: bool,
    written: bool,
) {
    // Too short to keep, or no history kept at all: nothing stays.
    if clip.duration_secs() < MIN_UTTERANCE_SECS || !inner.config.current().general.keep_history {
        if let Some(id) = held {
            drop_held(inner, id, path).await;
        } else if written {
            let _ = std::fs::remove_file(path);
        }
        return;
    }
    if let Some(id) = held {
        if trimmed {
            replace_held(inner, id, path, clip).await;
        }
        let history = inner.history.clone();
        let (raw, fin) = (texts.0.to_string(), texts.1.to_string());
        let audio_ms = clip_ms(clip);
        let marked =
            tokio::task::spawn_blocking(move || history.set_cancelled(id, &raw, &fin, audio_ms))
                .await;
        if !matches!(marked, Ok(Ok(()))) {
            tracing::warn!(path = %path.display(), "the cancelled dictation could not be marked");
        }
        return;
    }
    let row = crate::history::FailedUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        audio_path: (inner.config.current().general.keeps_recordings() || written)
            .then(|| path.to_path_buf()),
        audio_ms: clip_ms(clip),
        code: ErrorCode::Cancelled,
        error: crate::history::CANCELLED.into(),
        raw_text: texts.0.into(),
        final_text: texts.1.into(),
    };
    if !insert_with_recording(inner, row, clip, written).await {
        tracing::warn!(path = %path.display(), "the cancelled dictation could not be kept");
    }
}

/// The part of the recording that is processed and kept: `range` when
/// there is one. True when it is less than the whole.
fn cut(mut samples: Vec<f32>, range: Option<std::ops::Range<usize>>) -> (AudioClip, bool) {
    let mut trimmed = false;
    if let Some(range) = range {
        let end = range.end.min(samples.len());
        let start = range.start.min(end);
        if (start, end) != (0, samples.len()) {
            samples = samples[start..end].to_vec();
            trimmed = true;
        }
    }
    (
        AudioClip {
            samples,
            sample_rate: SAMPLE_RATE,
        },
        trimmed,
    )
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
    // The recording's file, written while the audio comes in.
    let mut journal: Option<crate::journal::Journal> = None;
    let range = loop {
        match cmd_rx.recv().await {
            Some(SessionCmd::Audio(chunk)) => {
                if journal.is_none() && held.is_none() {
                    if let Some((id, j)) = start_journal(&inner, &profile, &req, &wav_path).await {
                        held = Some(id);
                        journal = Some(j);
                    }
                }
                if let Some(j) = &journal {
                    j.push(chunk);
                }
            }
            Some(SessionCmd::Store) => {
                settle_journal(&inner, &mut journal, &mut held, &wav_path).await;
                if held.is_none() {
                    held = hold(&inner, &profile, &req, &audio, &wav_path, &flags).await;
                }
            }
            Some(SessionCmd::Finish(range)) => break range,
            None => {
                // Ended without `finish`. A kept cancel writes what was
                // recorded as a cancelled row; a plain cancel removes what
                // was kept; a session that was dropped leaves the row and
                // its recording, to be processed again from the history.
                if flags.keep.load(Ordering::SeqCst) {
                    let range = flags.keep_range.lock().unwrap().take();
                    settle_journal(&inner, &mut journal, &mut held, &wav_path).await;
                    if held.is_none() {
                        held = hold(&inner, &profile, &req, &audio, &wav_path, &flags).await;
                    }
                    let samples = std::mem::take(&mut *audio.lock().unwrap());
                    let (clip, trimmed) = cut(samples, range);
                    store_cancelled(
                        &inner,
                        &profile,
                        &req,
                        &clip,
                        held,
                        &wav_path,
                        ("", ""),
                        trimmed,
                        false,
                    )
                    .await;
                    return;
                }
                if let Some(j) = journal.take() {
                    j.close().await;
                }
                send_done_once(&tx, &flags.done_sent, &session_id, None).await;
                inner.schedule_idle_unload();
                return;
            }
        }
    };
    settle_journal(&inner, &mut journal, &mut held, &wav_path).await;
    if held.is_none() {
        held = hold(&inner, &profile, &req, &audio, &wav_path, &flags).await;
    }

    let samples = std::mem::take(&mut *audio.lock().unwrap());
    let recorded_ms = samples.len() as u64 * 1000 / SAMPLE_RATE as u64;
    let (clip, trimmed) = cut(samples, range);
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
        recognised: Default::default(),
        asr_sample: None,
    };

    let engines = inner.resolve_engines(&config, &profile);
    let pipeline = match Pipeline::build(&config, &profile, &engines, &inner.registry) {
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
    // A kept cancel stops the run here, with the transcript when the
    // recognition was done; a plain cancel aborts the whole task.
    let mut run = Box::pin(pipeline.run(clip.clone(), &ctx, &tx));
    let ran = tokio::select! {
        r = &mut run => r,
        _ = flags.cancel.notified() => {
            // Dropped first: a rewrite under way stops at once.
            drop(run);
            let raw = ctx.recognised.lock().unwrap().clone().unwrap_or_default();
            tracing::info!(session = %session_id, recognised = !raw.is_empty(), "cancelled while processing");
            store_cancelled(&inner, &profile, &req, &clip, held, &wav_path, (&raw, ""), false, false)
                .await;
            return;
        }
    };
    let outcome = match ran {
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
        if flags.keep.load(Ordering::SeqCst) {
            let texts = (outcome.raw_text.as_str(), outcome.final_text.as_str());
            store_cancelled(
                &inner, &profile, &req, &clip, held, &wav_path, texts, false, false,
            )
            .await;
        }
        return;
    }

    // A recording kept before the processing is already in place; `cancel`
    // removes it with its row. Otherwise it is written now.
    let audio_path = if held.is_some() {
        Some(wav_path.clone())
    } else if config.general.keeps_recordings() {
        let bytes = encode_wav_pcm16(&clip);
        let p = wav_path.clone();
        match tokio::task::spawn_blocking(move || std::fs::write(&p, bytes)).await {
            Ok(Ok(())) => Some(wav_path.clone()),
            Ok(Err(e)) => {
                tracing::warn!(path = %wav_path.display(), error = %e, "the recording could not be written");
                storage_failed(&tx, format!("the recording could not be written: {e}")).await;
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "the recording's write did not finish");
                storage_failed(&tx, "the recording could not be written".into()).await;
                None
            }
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
        if flags.keep.load(Ordering::SeqCst) {
            let texts = (outcome.raw_text.as_str(), outcome.final_text.as_str());
            store_cancelled(
                &inner,
                &profile,
                &req,
                &clip,
                held,
                &wav_path,
                texts,
                false,
                written_here.is_some(),
            )
            .await;
        } else if let Some(p) = &written_here {
            let _ = std::fs::remove_file(p);
        }
        return;
    }

    // The audio stages before the steps, the record stage after them; the
    // insertion stage comes from the client with the final text.
    let kept_ms = clip_ms(&clip).unwrap_or(0);
    let mut stages = vec![
        StageRecord {
            position: StageKind::Capture.position(false),
            kind: StageKind::Capture,
            text: None,
            ms: None,
            detail: serde_json::json!({ "duration_ms": recorded_ms }),
        },
        StageRecord {
            position: StageKind::Trim.position(false),
            kind: StageKind::Trim,
            text: None,
            ms: None,
            detail: serde_json::json!({ "removed_ms": recorded_ms.saturating_sub(kept_ms) }),
        },
    ];
    stages.extend(outcome.stages);
    stages.push(StageRecord {
        position: StageKind::Record.position(false),
        kind: StageKind::Record,
        text: None,
        ms: None,
        detail: serde_json::json!({ "history": true, "recording": audio_path.is_some() }),
    });
    // For a kept cancel that lands while the row is being written.
    let texts = (outcome.raw_text.clone(), outcome.final_text.clone());
    let new = NewUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        raw_text: outcome.raw_text,
        refined_text: outcome.final_text.clone(),
        final_text: outcome.final_text,
        degraded: outcome.degraded,
        audio_path,
        stages,
        asr_ms: outcome.asr_ms,
        llm_ms: outcome.llm_ms,
        audio_ms: clip_ms(&clip),
        dict_fixes: outcome.dict_fixes,
        snippets: outcome.snippets,
        llm_changed: outcome.llm_changed,
        dict_hits: outcome.dict_hits,
    };
    let history = inner.history.clone();
    let claim = flags.clone();
    let policy = inner.config.current().general.keep();
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
        let inserted = history.insert_replacing(
            new,
            held,
            || {
                let won = !claim.done_sent.swap(true, Ordering::SeqCst);
                claimed = Some(won);
                won
            },
            policy,
        );
        (inserted, claimed)
    })
    .await
    .unwrap_or((Ok(None), None));
    if let Err(e) = &inserted {
        tracing::warn!(session = %session_id, error = %e, "history insert failed");
        storage_failed(&tx, format!("the history row could not be written: {e}")).await;
    }
    let history_id = inserted.ok().flatten();

    // The insert can fail before it asks; the claim is then still open.
    let won = claimed.unwrap_or_else(|| !flags.done_sent.swap(true, Ordering::SeqCst));
    if !won {
        if flags.keep.load(Ordering::SeqCst) {
            let texts = (texts.0.as_str(), texts.1.as_str());
            store_cancelled(
                &inner,
                &profile,
                &req,
                &clip,
                held,
                &wav_path,
                texts,
                false,
                written_here.is_some(),
            )
            .await;
        } else if let Some(p) = &written_here {
            let _ = std::fs::remove_file(p);
        }
        tracing::info!(session = %session_id, "cancelled before the result was stored");
        return;
    }

    if let Some(id) = history_id {
        let mut sessions = inner.sessions.lock().unwrap();
        if sessions.len() >= SESSIONS_KEPT {
            sessions.clear();
        }
        sessions.insert(session_id.clone(), id);
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
    async fn llm_is_loaded_as_soon_as_a_recording_starts() {
        let (fake, _tmp, e) = idle_rig().await;
        let s = e.start_session(SessionRequest::default());
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fake.loads.load(Ordering::SeqCst), 1);
        s.cancel();
        drain(&s).await;
    }

    #[tokio::test(start_paused = true)]
    async fn llm_is_not_preloaded_when_the_setting_is_off() {
        let (fake, _tmp, e) = idle_rig().await;
        e.config()
            .update(|c| c.general.llm_preload_on_record = false)
            .unwrap();
        let s = e.start_session(SessionRequest::default());
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fake.loads.load(Ordering::SeqCst), 0);
        s.cancel();
        drain(&s).await;
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
        let models_dir = e.inner.dirs.models_dir();
        e.evict_model(&models_dir, "other").await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 0);
        e.evict_model(&models_dir, "m").await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 1);
    }

    // The fake factory hands out a fresh provider per build, so a provider
    // that was evicted comes back as a different instance.
    #[tokio::test(start_paused = true)]
    async fn unload_unreferenced_asr_evicts_only_unreferenced_providers() {
        let (_fake, _tmp, e) = idle_rig().await;
        let used = e.config().current().default_engines.asr.clone();
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
        assert!(model_predicate(&dirs.models_dir(), "custom/x.gguf")(&cfg));
        assert!(!model_predicate(&dirs.models_dir(), "custom/y.gguf")(&cfg));
        assert!(!model_predicate(&dirs.models_dir(), "x.gguf")(&cfg));
        cfg.model_path = Some(tmp.path().join("x.gguf"));
        assert!(!model_predicate(&dirs.models_dir(), "custom/x.gguf")(&cfg));
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
        mock_ok(&r, "汉字 原文", "汉字 整理後").await;
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
            |e| matches!(e, Event::Refined { text, degraded: false } if text == "漢字 整理後")
        ));
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        assert_eq!(row.raw_text, "汉字 原文");
        assert_eq!(row.final_text, "漢字 整理後");
        assert_eq!(row.app_name.as_deref(), Some("TextEdit"));
        assert!(row.audio_path.is_none());
        assert!(row.audio_ms.is_some());
        // Every stage but the insertion, which the client reports later.
        let stages = r.engine.history().stages(*id).unwrap();
        let positions: Vec<u32> = stages.iter().map(|s| s.position).collect();
        assert_eq!(positions, [1, 2, 3, 4, 5, 6, 7, 9]);
        assert!(stages[0].detail["duration_ms"].as_u64().unwrap() > 0);
        assert_eq!(stages[7].detail["recording"], false);

        r.engine
            .record_final(
                s.id(),
                "使用者改過",
                Some(Delivery {
                    method: "paste".into(),
                    outcome: "inserted".into(),
                    app_name: Some("TextEdit".into()),
                }),
            )
            .await
            .unwrap();
        assert_eq!(
            r.engine.history().get(*id).unwrap().unwrap().final_text,
            "使用者改過"
        );
        let (last, stages) = r
            .engine
            .last_run(&ProfileId("default".into()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(last.id, *id);
        assert_eq!(stages.len(), 9);
        assert_eq!(stages[7].kind, StageKind::Insert);
        assert_eq!(stages[7].detail["method"], "paste");
        assert!(r
            .engine
            .record_final(&SessionId("nope".into()), "x", None)
            .await
            .is_err());
        // The session is forgotten once its final text is recorded.
        assert!(matches!(
            r.engine.record_final(s.id(), "再一次", None).await,
            Err(EngineError::UnknownSession(_))
        ));
        assert!(r.engine.inner.sessions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sessions_never_told_their_final_text_do_not_pile_up() {
        let r = rig(false).await;
        mock_ok(&r, "原文", "整理後").await;
        {
            let mut map = r.engine.inner.sessions.lock().unwrap();
            for i in 0..SESSIONS_KEPT {
                map.insert(SessionId(format!("old-{i}")), HistoryId(i as i64 + 1000));
            }
        }
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: Some(_),
                ..
            })
        ));
        let map = r.engine.inner.sessions.lock().unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(s.id()));
    }

    #[tokio::test]
    async fn a_failed_blocking_task_is_an_error_not_a_panic() {
        let join = tokio::task::spawn_blocking(|| panic!("boom"))
            .await
            .unwrap_err();
        let e = EngineError::from(join);
        assert!(matches!(e, EngineError::Task(_)), "{e}");
        assert!(e.to_string().contains("boom"), "{e}");
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

    /// The events after a result whose storage failed: the error that says
    /// so, then `Done` without a row.
    fn storage_failed_then_done(events: &[Event]) -> bool {
        let n = events.len();
        n >= 2
            && matches!(
                events[n - 2],
                Event::Error {
                    code: ErrorCode::StorageFailed,
                    recoverable: true,
                    ..
                }
            )
            && matches!(
                events[n - 1],
                Event::Done {
                    history_id: None,
                    ..
                }
            )
    }

    #[tokio::test]
    async fn a_recording_that_cannot_be_written_is_reported_before_done() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        // A file where the recordings folder should be: every write fails.
        let dir = r.engine.recordings_dir();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::write(&dir, b"").unwrap();
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        let n = events.len();
        assert!(
            matches!(
                events[n - 2],
                Event::Error {
                    code: ErrorCode::StorageFailed,
                    recoverable: true,
                    ..
                }
            ),
            "{events:?}"
        );
        // The text is still the result; only the recording is missing.
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        assert!(r
            .engine
            .history()
            .get(*id)
            .unwrap()
            .unwrap()
            .audio_path
            .is_none());
    }

    #[tokio::test]
    async fn a_row_that_cannot_be_written_is_reported_before_done() {
        let r = rig(false).await;
        mock_ok(&r, "原文", "整理後").await;
        r.engine
            .history()
            .connection()
            .execute_batch("DROP TABLE utterances")
            .unwrap();
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(storage_failed_then_done(&events), "{events:?}");
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
        assert_eq!(done.attempt.final_text, "整理後");
        assert_eq!(done.attempt.raw_text, "原文");
        assert_eq!(done.attempt.profile_id, row.profile_id);
        assert!(done.attempt.llm_label.is_some());
        // A row that had failed takes the result at once.
        assert!(done.chosen);
        let row = r.engine.history().get(row.id).unwrap().unwrap();
        assert_eq!(
            (row.error, row.final_text.as_str(), row.redone),
            (None, "整理後", 2)
        );
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 1);
        // The failed run before it is in the record too, after the failure the row had.
        let attempts = r.engine.history().attempts(row.id).unwrap();
        assert_eq!(attempts.len(), 3);
        assert_eq!(attempts[1].error_code.as_deref(), Some("provider_error"));
        assert!(attempts[2].origin);
    }

    #[tokio::test]
    async fn a_failed_dictation_leaves_a_row_without_a_recording_when_recordings_are_not_kept() {
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
        let rows = r.engine.history().list_recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].error_code.as_deref(), Some("provider_error"));
        assert!(rows[0].audio_path.is_none());
        assert_eq!(rows[0].audio_ms, Some(1000));
        // Without a recording there is nothing to process again.
        assert!(matches!(
            r.engine.reprocess(rows[0].id, None).await,
            Err(ReprocessError::NoRecording)
        ));
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 0);
    }

    /// The rows of the history once `n` are there, within a few seconds:
    /// a kept cancel writes its row after `Done`.
    async fn rows_after_cancel(r: &Rig, n: usize) -> Vec<crate::history::Utterance> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let rows = r.engine.history().list_recent(10).unwrap();
            if rows.len() >= n
                && rows
                    .iter()
                    .all(|u| u.error_code.as_deref() != Some("interrupted"))
            {
                return rows;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no row after the cancel"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn a_kept_cancel_while_recording_leaves_a_cancelled_row_with_the_recording() {
        let r = rig(true).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.push_audio(&one_second());
        // The second half holds the speech.
        s.cancel_keep(Some(16_000..32_000));
        let events = drain(&s).await;
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
        let rows = rows_after_cancel(&r, 1).await;
        let row = &rows[0];
        assert_eq!(row.error_code.as_deref(), Some("cancelled"));
        assert_eq!(row.audio_ms, Some(1000));
        assert_eq!(row.final_text, "");
        let path = row.audio_path.clone().expect("recording kept");
        assert!(path.exists());
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 0);
        // It can be processed again from there.
        mock_ok(&r, "原文", "整理後").await;
        let done = r.engine.reprocess(row.id, None).await.unwrap();
        assert!(done.chosen);
        let row = r.engine.history().get(row.id).unwrap().unwrap();
        assert_eq!((row.error, row.final_text.as_str()), (None, "整理後"));
    }

    #[tokio::test]
    async fn a_kept_cancel_while_recording_leaves_a_row_without_a_recording_when_not_kept() {
        let r = rig(false).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.cancel_keep(None);
        drain(&s).await;
        let rows = rows_after_cancel(&r, 1).await;
        assert_eq!(rows[0].error_code.as_deref(), Some("cancelled"));
        assert!(rows[0].audio_path.is_none());
        assert_eq!(rows[0].audio_ms, Some(1000));
    }

    #[tokio::test]
    async fn a_kept_cancel_of_a_recording_too_short_to_hold_speech_leaves_nothing() {
        let r = rig(true).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second()[..1600]);
        s.cancel_keep(None);
        drain(&s).await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
        assert!(std::fs::read_dir(r.engine.recordings_dir())
            .map(|d| d.count() == 0)
            .unwrap_or(true));
    }

    #[tokio::test]
    async fn a_kept_cancel_while_the_rewrite_runs_keeps_the_transcript() {
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
                    .set_delay(std::time::Duration::from_millis(800)),
            )
            .mount(&r.llm)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();
        while let Some(e) = stream.next().await {
            if matches!(e, Event::Transcript { is_final: true, .. }) {
                break;
            }
        }
        s.cancel_keep(None);
        let rows = rows_after_cancel(&r, 1).await;
        let row = &rows[0];
        assert_eq!(row.error_code.as_deref(), Some("cancelled"));
        assert_eq!(row.raw_text, "原文");
        assert_eq!(row.final_text, "");
        assert!(row.audio_path.is_some());
        // The rewrite was not waited for.
        assert!(r.llm.received_requests().await.unwrap().len() <= 1);
        drop(stream);
        drop(s);
    }

    #[tokio::test]
    async fn a_kept_cancel_during_the_tail_keeps_the_whole_result_as_cancelled() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        r.engine.inner.tail_delay_ms.store(300, Ordering::SeqCst);
        let s = r.engine.start_session(SessionRequest::default());
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();
        while let Some(e) = stream.next().await {
            if matches!(e, Event::Refined { .. }) {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        s.cancel_keep(None);
        drop(stream);
        drop(s);
        let rows = rows_after_cancel(&r, 1).await;
        let row = &rows[0];
        assert_eq!(row.error_code.as_deref(), Some("cancelled"));
        assert_eq!(
            (row.raw_text.as_str(), row.final_text.as_str()),
            ("原文", "整理後")
        );
        assert!(row.audio_path.is_some());
        assert_eq!(r.engine.history().stats(0, 0).unwrap().totals.count, 0);
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
        let rows = rows_when(&r, |rows| rows.len() == 1 && rows[0].audio_ms.is_some()).await;
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

    /// Waits until the recording file holds at least `secs` of audio.
    async fn wav_secs_when(path: &Path, secs: f32) -> f32 {
        for _ in 0..300 {
            if let Some(ms) = crate::audio::wav_duration_ms(path) {
                let got = ms as f32 / 1000.0;
                if got >= secs {
                    return got;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("the recording never reached {secs} s");
    }

    #[tokio::test]
    async fn a_recording_is_on_disk_while_it_is_being_made() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        // Pushed in the small blocks a microphone delivers.
        for chunk in one_second().chunks(1600) {
            s.push_audio(chunk);
        }
        // The row and the file exist before the recording is finished.
        let rows = rows_when(&r, |rows| rows.len() == 1).await;
        assert_eq!(rows[0].error_code.as_deref(), Some("interrupted"));
        let path = rows[0].audio_path.clone().unwrap();
        assert!(wav_secs_when(&path, 0.9).await >= 0.9);
        // More audio, then the end: the result replaces the row and the file holds all of it.
        for chunk in one_second().chunks(1600) {
            s.push_audio(chunk);
        }
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
        assert_eq!(row.error_code, None);
        assert_eq!(row.audio_path.as_deref(), Some(path.as_path()));
        assert!((wav_secs_when(&path, 1.9).await - 2.0).abs() < 0.05);
        assert_eq!(r.engine.history().list_recent(10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn audio_past_the_limit_is_dropped() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        let mut s = r.engine.start_session(SessionRequest::default());
        s.max_samples = SAMPLE_RATE as usize * 2;
        for _ in 0..3 {
            for chunk in one_second().chunks(1600) {
                s.push_audio(chunk);
            }
        }
        assert_eq!(s.audio.lock().unwrap().len(), SAMPLE_RATE as usize * 2);
        let rows = rows_when(&r, |rows| rows.len() == 1).await;
        let path = rows[0].audio_path.clone().unwrap();
        assert!((wav_secs_when(&path, 1.9).await - 2.0).abs() < 0.05);
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
        assert!((row.audio_ms.unwrap() as f32 / 1000.0 - 2.0).abs() < 0.05);
        assert!((wav_secs_when(&path, 1.9).await - 2.0).abs() < 0.05);
    }

    #[tokio::test]
    async fn a_cancel_while_recording_leaves_nothing() {
        let r = rig(true).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        let rows = rows_when(&r, |rows| rows.len() == 1).await;
        let path = rows[0].audio_path.clone().unwrap();
        wav_secs_when(&path, 0.9).await;
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
    async fn a_session_dropped_while_recording_leaves_a_whole_file() {
        let r = rig(true).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        let rows = rows_when(&r, |rows| rows.len() == 1).await;
        let path = rows[0].audio_path.clone().unwrap();
        wav_secs_when(&path, 0.9).await;
        drop(s);
        for _ in 0..100 {
            if !r.engine.in_flight(&path) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let rows = r.engine.history().list_recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].error_code.as_deref(), Some("interrupted"));
        assert_eq!(rows[0].audio_ms, Some(1000));
        assert!(crate::audio::decode_wav_pcm16(&std::fs::read(&path).unwrap()).is_ok());
    }

    #[tokio::test]
    async fn a_rerun_without_a_choice_uses_the_profile_of_the_dictation() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        r.engine
            .config()
            .update(|c| {
                let mut other = c.profiles[0].clone();
                other.id = ProfileId("other".into());
                other.name = "Other".into();
                other.is_default = false;
                c.profiles.push(other);
            })
            .unwrap();
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
        let own = r.engine.history().get(*id).unwrap().unwrap().profile_id;
        let done = r
            .engine
            .reprocess(*id, Some(ProfileId("other".into())))
            .await
            .unwrap();
        assert!(r
            .engine
            .history()
            .choose_attempt(*id, done.attempt.id)
            .unwrap());
        assert_eq!(
            r.engine.history().get(*id).unwrap().unwrap().profile_id.0,
            "other"
        );
        // No choice: the profile of the dictation, not of the result in use.
        let again = r.engine.reprocess(*id, None).await.unwrap();
        assert_eq!(again.attempt.profile_id, own);
    }

    #[tokio::test]
    async fn a_session_dropped_after_store_leaves_its_recording_to_retry() {
        let r = rig(true).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.store();
        drop(s);
        let rows = rows_when(&r, |rows| rows.len() == 1 && rows[0].audio_ms.is_some()).await;
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
        assert_eq!(again.attempt.final_text, "整理後");
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
            dict_hits: Vec::new(),
            stages: Vec::new(),
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
        a.backup_write_folder(&folder, text_backup(), None, false)
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
        b.backup_write_folder(&folder, text_backup(), None, false)
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
        let got = a
            .backup_write_folder(&folder, text_backup(), None, false)
            .await;
        assert!(matches!(
            got,
            Err(EngineError::Backup(BackupError::Io { .. }))
        ));
        assert!(!tmp.path().join("unmounted").exists());
    }

    #[tokio::test]
    async fn a_backup_folder_that_is_gone_is_made_again_when_asked() {
        let tmp = tempfile::tempdir().unwrap();
        let a = plain_engine(&tmp.path().join("a")).await;
        a.history().insert(row(1_790_812_800, "one")).unwrap();
        let folder = tmp.path().join("sync");
        let first = a
            .backup_write_folder(&folder, text_backup(), None, true)
            .await
            .unwrap();
        assert!(first.recreated);
        assert_eq!(first.rows, 1);
        assert!(folder.join("devices").is_dir());
        // The folder is there now: the next write is an ordinary one.
        let second = a
            .backup_write_folder(&folder, text_backup(), None, true)
            .await
            .unwrap();
        assert!(!second.recreated);
    }

    #[tokio::test]
    async fn import_is_refused_while_a_dictation_is_in_flight() {
        let tmp = tempfile::tempdir().unwrap();
        let a = plain_engine(&tmp.path().join("a")).await;
        a.history().insert(row(1_790_812_800, "one")).unwrap();
        let folder = tmp.path().join("sync");
        std::fs::create_dir_all(&folder).unwrap();
        a.backup_write_folder(&folder, text_backup(), None, false)
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
