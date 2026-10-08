//! Keeps the backup folder up to date: a few minutes after a change, never
//! while a dictation is being processed, and once more before the app ends.

use crate::commands::CmdError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arcvox_core::backup::{FolderState, WriteReport};
use serde::{Deserialize, Serialize};

/// How long after the last change the folder is written.
pub const DELAY: Duration = Duration::from_secs(180);
/// How often the worker looks whether a write is due.
const TICK: Duration = Duration::from_secs(1);
/// How long quitting waits for the last write.
const FLUSH_LIMIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BackupStatus {
    /// Nothing was written to the folder yet.
    Never,
    Ok {
        at: i64,
        /// What that write added or changed in the folder.
        report: WriteReport,
    },
    Failed {
        at: i64,
        reason: String,
        /// The failure with its code, when it has one the interface can word.
        error: Option<CmdError>,
        /// The last write that worked.
        last_ok: Option<i64>,
        /// What the folder was found to be, when that is why.
        folder: Option<FolderState>,
    },
    Running,
}

/// The last write that worked, kept in `backup-status.json` so the page still
/// names it after a relaunch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastWrite {
    /// The folder it went to; a write to another folder says nothing about
    /// the one chosen now.
    pub folder: PathBuf,
    pub at: i64,
    pub report: WriteReport,
}

impl LastWrite {
    const FILE_NAME: &str = "backup-status.json";

    pub fn load(config_dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(config_dir.join(Self::FILE_NAME)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let tmp = config_dir.join(format!("{}.tmp", Self::FILE_NAME));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, config_dir.join(Self::FILE_NAME))
    }

    /// The status this stands for while `folder` is the one chosen.
    pub fn status_for(self, folder: Option<&Path>) -> Option<BackupStatus> {
        (folder == Some(self.folder.as_path())).then_some(BackupStatus::Ok {
            at: self.at,
            report: self.report,
        })
    }
}

/// When the next write is due.
#[derive(Debug, Default)]
pub struct Schedule {
    due: Option<Instant>,
}

impl Schedule {
    /// Something changed: the write moves to `DELAY` from now.
    pub fn touch(&mut self, now: Instant) {
        self.due = Some(now + DELAY);
    }

    /// True once when the delay has passed and nothing is being processed.
    pub fn take_due(&mut self, now: Instant, busy: bool) -> bool {
        match self.due {
            Some(due) if now >= due && !busy => {
                self.due = None;
                true
            }
            _ => false,
        }
    }

    pub fn pending(&self) -> bool {
        self.due.is_some()
    }
}

/// Why a write did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub reason: String,
    /// The failure with its code, when it has one.
    pub error: Option<CmdError>,
    /// What was found when the folder itself is the cause.
    pub folder: Option<FolderState>,
}

impl Failure {
    pub fn other(reason: impl Into<String>) -> Self {
        Failure {
            reason: reason.into(),
            error: None,
            folder: None,
        }
    }

    /// A failure the interface words by its code.
    pub fn error(error: CmdError) -> Self {
        Failure {
            reason: error.message.clone(),
            error: Some(error),
            folder: None,
        }
    }

    pub fn folder(state: FolderState) -> Self {
        let reason = match state {
            FolderState::Missing => "the folder does not exist",
            FolderState::Blocked => "the system does not let the app into the folder",
            _ => "the folder cannot be written",
        };
        Failure {
            reason: reason.to_string(),
            error: None,
            folder: Some(state),
        }
    }
}

/// What the folder is taken to be after a look at it. A look that did not
/// try to write cannot clear a refusal a write ran into.
pub fn merged(current: FolderState, seen: FolderState, probed: bool) -> FolderState {
    let refused = matches!(current, FolderState::NotWritable | FolderState::Blocked);
    if !probed && seen == FolderState::Usable && refused {
        current
    } else {
        seen
    }
}

/// Writes the folder. The argument is true for a write the user asked for.
/// `Ok(None)` means there was nothing to do (no folder, or switched off).
pub type Run = Box<dyn Fn(bool) -> Result<Option<WriteReport>, Failure> + Send + Sync>;
/// Told when the folder's state changes.
pub type Look = Box<dyn Fn(FolderState) + Send + Sync>;
pub type Busy = Box<dyn Fn() -> bool + Send + Sync>;
pub type Notify = Box<dyn Fn(&BackupStatus) + Send + Sync>;

enum Msg {
    Touch,
    RunNow,
    /// The folder can be used again: what was waiting is written, as an
    /// automatic write.
    Retry,
}

struct Shared {
    run: Run,
    busy: Busy,
    notify: Notify,
    status: Mutex<BackupStatus>,
    /// What the folder was last found to be.
    folder: Mutex<FolderState>,
    look: Mutex<Option<Look>>,
    /// A change is waiting for its write.
    pending: AtomicBool,
    /// One write at a time.
    writing: Mutex<()>,
}

/// A lock is still usable after a write panicked while holding it.
fn held<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Shared {
    fn set(&self, status: BackupStatus) {
        *held(&self.status) = status.clone();
        (self.notify)(&status);
    }

    /// Notes what the folder was found to be; tells the listener on a change.
    fn saw(&self, state: FolderState) {
        let before = std::mem::replace(&mut *held(&self.folder), state);
        if before != state {
            if let Some(look) = held(&self.look).as_ref() {
                look(state);
            }
        }
    }

    /// Writes once the write before it has ended. With `only_pending`, only
    /// when a change still waits for its write.
    fn write(&self, manual: bool, only_pending: bool) {
        let _one = held(&self.writing);
        if !self.pending.swap(false, AtomicOrdering::SeqCst) && only_pending {
            return;
        }
        let before = held(&self.status).clone();
        self.set(BackupStatus::Running);
        let at = arcvox_core::history::now_unix();
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.run)(manual)))
            .unwrap_or_else(|_| {
                Err(Failure::error(CmdError::with(
                    "internal",
                    "the write stopped unexpectedly",
                    &[("error", "the write stopped unexpectedly".to_string())],
                )))
            });
        match &ran {
            Ok(Some(_)) => self.saw(FolderState::Usable),
            Err(Failure {
                folder: Some(state),
                ..
            }) => self.saw(*state),
            _ => {}
        }
        let next = match ran {
            Ok(Some(report)) => BackupStatus::Ok { at, report },
            Ok(None) => before,
            Err(Failure {
                reason,
                error,
                folder,
            }) => BackupStatus::Failed {
                at,
                reason,
                error,
                folder,
                last_ok: match before {
                    BackupStatus::Ok { at, .. } => Some(at),
                    BackupStatus::Failed { last_ok, .. } => last_ok,
                    _ => None,
                },
            },
        };
        self.set(next);
    }
}

#[derive(Clone)]
pub struct BackupAuto {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
}

impl BackupAuto {
    pub fn spawn(run: Run, busy: Busy, notify: Notify) -> BackupAuto {
        let shared = Arc::new(Shared {
            run,
            busy,
            notify,
            status: Mutex::new(BackupStatus::Never),
            folder: Mutex::new(FolderState::Usable),
            look: Mutex::new(None),
            pending: AtomicBool::new(false),
            writing: Mutex::new(()),
        });
        let (tx, rx) = mpsc::channel();
        let worker = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("arcvox-backup".into())
            .spawn(move || {
                let mut schedule = Schedule::default();
                let mut asked = false;
                let mut again = false;
                loop {
                    match rx.recv_timeout(TICK) {
                        Ok(Msg::Touch) => schedule.touch(Instant::now()),
                        Ok(Msg::RunNow) => asked = true,
                        Ok(Msg::Retry) => again = true,
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    // A flush took the write that was waiting.
                    if schedule.pending() && !asked && !worker.pending.load(AtomicOrdering::SeqCst)
                    {
                        schedule = Schedule::default();
                    }
                    let busy = (worker.busy)();
                    if asked && !busy {
                        asked = false;
                        schedule = Schedule::default();
                        worker.write(true, false);
                    } else if again && !busy {
                        again = false;
                        schedule = Schedule::default();
                        worker.write(false, false);
                    } else if schedule.take_due(Instant::now(), busy) {
                        worker.write(false, false);
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "the backup worker did not start");
        }
        BackupAuto { tx, shared }
    }

    /// Something that belongs in the backup changed.
    pub fn touch(&self) {
        self.shared.pending.store(true, AtomicOrdering::SeqCst);
        let _ = self.tx.send(Msg::Touch);
    }

    /// Writes as soon as no dictation is being processed.
    pub fn run_now(&self) {
        self.shared.pending.store(true, AtomicOrdering::SeqCst);
        let _ = self.tx.send(Msg::RunNow);
    }

    /// For the moment before the process ends: waits for a write that is
    /// running, then writes when a change is still waiting.
    pub fn flush(&self) {
        self.flush_within(FLUSH_LIMIT);
    }

    /// `flush` that gives up after `limit`, so a folder that does not answer
    /// cannot keep the app from quitting.
    pub fn flush_within(&self, limit: Duration) {
        let shared = self.shared.clone();
        let (done, wait) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("arcvox-backup-flush".into())
            .spawn(move || {
                shared.write(false, true);
                let _ = done.send(());
            });
        if spawned.is_ok() {
            let _ = wait.recv_timeout(limit);
        }
    }

    /// Starts from the status an earlier launch left, unless a write has
    /// already said something newer.
    pub fn restore(&self, status: BackupStatus) {
        let mut current = held(&self.shared.status);
        if *current == BackupStatus::Never {
            *current = status;
        }
    }

    pub fn status(&self) -> BackupStatus {
        held(&self.shared.status).clone()
    }

    /// Who is told when the folder's state changes.
    pub fn on_folder(&self, look: Look) {
        *held(&self.shared.look) = Some(look);
    }

    /// What the folder was last found to be.
    pub fn folder(&self) -> FolderState {
        *held(&self.shared.folder)
    }

    /// Takes a look at the folder made outside a write; `probed` says a
    /// file was written to find out. Returns what the folder counts as now.
    pub fn saw(&self, seen: FolderState, probed: bool) -> FolderState {
        let before = self.folder();
        let state = merged(before, seen, probed);
        self.shared.saw(state);
        if !before.usable() && state.usable() {
            self.write_soon();
        }
        state
    }

    /// An automatic write as soon as no dictation is being processed,
    /// without waiting for a change: the folder came back, or it is gone
    /// and is to be made again.
    pub fn write_soon(&self) {
        self.shared.pending.store(true, AtomicOrdering::SeqCst);
        let _ = self.tx.send(Msg::Retry);
    }

    /// `saw` for a caller that knows the settings: a folder newly found
    /// gone is made again at once when `recreate` says so, instead of at
    /// the next change. Only the moment it is found gone counts, so a folder
    /// that cannot be made is not tried again with every look.
    pub fn saw_with(&self, seen: FolderState, probed: bool, recreate: bool) -> FolderState {
        let before = self.folder();
        let state = self.saw(seen, probed);
        if recreate && state == FolderState::Missing && before != FolderState::Missing {
            self.write_soon();
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_change_is_due_after_the_delay() {
        let t0 = Instant::now();
        let mut s = Schedule::default();
        assert!(!s.take_due(t0 + DELAY * 2, false));
        s.touch(t0);
        assert!(s.pending());
        assert!(!s.take_due(t0 + DELAY - Duration::from_secs(1), false));
        assert!(s.take_due(t0 + DELAY, false));
        assert!(!s.pending());
        assert!(!s.take_due(t0 + DELAY * 2, false));
    }

    #[test]
    fn changes_in_a_row_make_one_write() {
        let t0 = Instant::now();
        let mut s = Schedule::default();
        for secs in [0, 60, 120] {
            s.touch(t0 + Duration::from_secs(secs));
        }
        assert!(!s.take_due(t0 + Duration::from_secs(200), false));
        assert!(s.take_due(t0 + Duration::from_secs(300), false));
        assert!(!s.take_due(t0 + Duration::from_secs(600), false));
    }

    #[test]
    fn no_write_while_a_dictation_is_in_flight() {
        let t0 = Instant::now();
        let mut s = Schedule::default();
        s.touch(t0);
        assert!(!s.take_due(t0 + DELAY, true));
        assert!(s.pending());
        assert!(s.take_due(t0 + DELAY + Duration::from_secs(1), false));
    }

    fn wait_for(auto: &BackupAuto, done: impl Fn(&BackupStatus) -> bool) -> BackupStatus {
        for _ in 0..200 {
            let s = auto.status();
            if done(&s) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the status did not change: {:?}", auto.status());
    }

    #[test]
    fn a_failed_write_keeps_the_reason_and_runs_again() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (c, s) = (calls.clone(), seen.clone());
        let auto = BackupAuto::spawn(
            Box::new(move |manual| {
                assert!(manual);
                match c.fetch_add(1, Ordering::SeqCst) {
                    0 => Err(Failure::other("the folder is gone")),
                    _ => Ok(Some(WriteReport::default())),
                }
            }),
            Box::new(|| false),
            Box::new(move |status| s.lock().unwrap().push(status.clone())),
        );
        assert_eq!(auto.status(), BackupStatus::Never);
        auto.run_now();
        let failed = wait_for(&auto, |s| matches!(s, BackupStatus::Failed { .. }));
        match failed {
            BackupStatus::Failed {
                reason, last_ok, ..
            } => {
                assert_eq!(reason, "the folder is gone");
                assert_eq!(last_ok, None);
            }
            other => panic!("{other:?}"),
        }
        auto.run_now();
        wait_for(&auto, |s| matches!(s, BackupStatus::Ok { .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let seen = seen.lock().unwrap();
        assert!(seen.contains(&BackupStatus::Running));
        assert!(matches!(seen.last(), Some(BackupStatus::Ok { .. })));
    }

    #[test]
    fn a_failure_after_a_success_remembers_the_success() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |_| match c.fetch_add(1, Ordering::SeqCst) {
                0 => Ok(Some(WriteReport::default())),
                _ => Err(Failure::other("full")),
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.run_now();
        let at = match wait_for(&auto, |s| matches!(s, BackupStatus::Ok { .. })) {
            BackupStatus::Ok { at, .. } => at,
            other => panic!("{other:?}"),
        };
        auto.run_now();
        let failed = wait_for(&auto, |s| matches!(s, BackupStatus::Failed { .. }));
        assert!(matches!(failed, BackupStatus::Failed { last_ok: Some(t), .. } if t == at));
    }

    #[test]
    fn the_status_says_what_the_write_changed() {
        let written = WriteReport {
            rows: 3,
            cells: 2,
            settings: true,
            ..WriteReport::default()
        };
        let w = written.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |_| Ok(Some(w.clone()))),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.run_now();
        let status = wait_for(&auto, |s| matches!(s, BackupStatus::Ok { .. }));
        assert!(matches!(status, BackupStatus::Ok { report, .. } if report == written));
    }

    #[test]
    fn the_last_write_is_kept_for_the_folder_it_went_to() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(LastWrite::load(dir.path()), None);
        let last = LastWrite {
            folder: PathBuf::from("/sync/arcvox"),
            at: 42,
            report: WriteReport {
                rows: 3,
                ..WriteReport::default()
            },
        };
        last.save(dir.path()).unwrap();
        assert!(!dir.path().join("backup-status.json.tmp").exists());
        assert_eq!(LastWrite::load(dir.path()), Some(last.clone()));
        assert_eq!(
            last.clone().status_for(Some(Path::new("/sync/arcvox"))),
            Some(BackupStatus::Ok {
                at: 42,
                report: last.report.clone()
            })
        );
        assert_eq!(last.clone().status_for(Some(Path::new("/other"))), None);
        assert_eq!(last.status_for(None), None);
        std::fs::write(dir.path().join("backup-status.json"), b"{ not json").unwrap();
        assert_eq!(LastWrite::load(dir.path()), None);
    }

    #[test]
    fn a_look_without_a_write_does_not_clear_a_refusal() {
        use FolderState::*;
        assert_eq!(merged(NotWritable, Usable, false), NotWritable);
        assert_eq!(merged(Blocked, Usable, false), Blocked);
        assert_eq!(merged(NotWritable, Usable, true), Usable);
        assert_eq!(merged(NotWritable, Missing, false), Missing);
        assert_eq!(merged(Missing, Usable, false), Usable);
        assert_eq!(merged(Usable, Missing, false), Missing);
    }

    #[test]
    fn a_write_the_folder_refuses_says_so_until_one_works() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |_| match c.fetch_add(1, Ordering::SeqCst) {
                0 => Err(Failure::folder(FolderState::Missing)),
                _ => Ok(Some(WriteReport::default())),
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        auto.on_folder(Box::new(move |state| s.lock().unwrap().push(state)));
        assert_eq!(auto.folder(), FolderState::Usable);
        auto.run_now();
        wait_for(&auto, |s| matches!(s, BackupStatus::Failed { .. }));
        assert_eq!(auto.folder(), FolderState::Missing);
        auto.run_now();
        wait_for(&auto, |s| matches!(s, BackupStatus::Ok { .. }));
        assert_eq!(auto.folder(), FolderState::Usable);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![FolderState::Missing, FolderState::Usable]
        );
    }

    #[test]
    fn a_folder_that_comes_back_is_written_to_as_an_automatic_write() {
        let manual = Arc::new(Mutex::new(Vec::new()));
        let m = manual.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |by_hand| {
                m.lock().unwrap().push(by_hand);
                Ok(Some(WriteReport::default()))
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        // Nothing to make up for while the folder was never found unusable.
        auto.saw(FolderState::Usable, true);
        auto.saw(FolderState::Missing, false);
        auto.saw(FolderState::Usable, false);
        wait_for(&auto, |s| matches!(s, BackupStatus::Ok { .. }));
        assert_eq!(*manual.lock().unwrap(), vec![false]);
    }

    #[test]
    fn a_folder_found_gone_is_written_to_at_once_when_it_is_to_be_made_again() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |by_hand| {
                assert!(!by_hand);
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        let settle = || std::thread::sleep(Duration::from_millis(1500));
        // Not asked for: nothing is written.
        auto.saw_with(FolderState::Missing, false, false);
        settle();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // Asked for, but the folder was known to be gone already.
        auto.saw_with(FolderState::Missing, false, true);
        settle();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // It comes back (one write), then is found gone: one more, and no
        // further ones while it stays gone.
        auto.saw(FolderState::Usable, false);
        settle();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        for _ in 0..3 {
            auto.saw_with(FolderState::Missing, false, true);
        }
        settle();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_failure_names_the_folder_when_that_is_why() {
        let auto = BackupAuto::spawn(
            Box::new(|_| Err(Failure::folder(FolderState::NotWritable))),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.run_now();
        let failed = wait_for(&auto, |s| matches!(s, BackupStatus::Failed { .. }));
        assert!(matches!(
            failed,
            BackupStatus::Failed {
                folder: Some(FolderState::NotWritable),
                ..
            }
        ));
    }

    #[test]
    fn a_look_from_outside_is_told_once_per_change() {
        let auto = BackupAuto::spawn(Box::new(|_| Ok(None)), Box::new(|| false), Box::new(|_| {}));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        auto.on_folder(Box::new(move |state| s.lock().unwrap().push(state)));
        assert_eq!(auto.saw(FolderState::Missing, false), FolderState::Missing);
        assert_eq!(auto.saw(FolderState::Missing, false), FolderState::Missing);
        assert_eq!(auto.saw(FolderState::Usable, false), FolderState::Usable);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![FolderState::Missing, FolderState::Usable]
        );
    }

    #[test]
    fn a_restored_status_does_not_replace_a_newer_one() {
        let old = BackupStatus::Ok {
            at: 1,
            report: WriteReport::default(),
        };
        let auto = BackupAuto::spawn(
            Box::new(|_| Err(Failure::other("full"))),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.restore(old.clone());
        assert_eq!(auto.status(), old);
        auto.run_now();
        let failed = wait_for(&auto, |s| matches!(s, BackupStatus::Failed { .. }));
        assert!(matches!(
            failed,
            BackupStatus::Failed {
                last_ok: Some(1),
                ..
            }
        ));
        auto.restore(old);
        assert!(matches!(auto.status(), BackupStatus::Failed { .. }));
    }

    #[test]
    fn a_run_with_nothing_to_do_leaves_the_status() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.touch();
        auto.flush();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(auto.status(), BackupStatus::Never);
    }

    #[test]
    fn flush_waits_for_a_write_that_is_running() {
        let finished = Arc::new(AtomicUsize::new(0));
        let f = finished.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |_| {
                std::thread::sleep(Duration::from_millis(400));
                f.fetch_add(1, Ordering::SeqCst);
                Ok(Some(WriteReport::default()))
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.run_now();
        wait_for(&auto, |s| *s == BackupStatus::Running);
        auto.flush();
        assert_eq!(finished.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_write_that_panics_does_not_break_the_next_one() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |_| match c.fetch_add(1, Ordering::SeqCst) {
                0 => panic!("boom"),
                _ => Ok(Some(WriteReport::default())),
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.touch();
        auto.flush();
        assert!(matches!(auto.status(), BackupStatus::Failed { .. }));
        auto.touch();
        auto.flush();
        assert!(matches!(auto.status(), BackupStatus::Ok { .. }));
    }

    #[test]
    fn flush_gives_up_on_a_write_that_does_not_return() {
        let auto = BackupAuto::spawn(
            Box::new(move |_| {
                std::thread::sleep(Duration::from_secs(5));
                Ok(Some(WriteReport::default()))
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.touch();
        let started = Instant::now();
        auto.flush_within(Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn flush_runs_a_pending_write_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let auto = BackupAuto::spawn(
            Box::new(move |manual| {
                assert!(!manual);
                c.fetch_add(1, Ordering::SeqCst);
                Ok(Some(WriteReport::default()))
            }),
            Box::new(|| false),
            Box::new(|_| {}),
        );
        auto.flush();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        auto.touch();
        auto.flush();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        auto.flush();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(auto.status(), BackupStatus::Ok { .. }));
    }
}
