//! Keeps the backup folder up to date: a few minutes after a change, never
//! while a dictation is being processed, and once more before the app ends.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arcvox_core::backup::WriteReport;
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
        /// The last write that worked.
        last_ok: Option<i64>,
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

/// Writes the folder. The argument is true for a write the user asked for.
/// `Ok(None)` means there was nothing to do (no folder, or switched off).
pub type Run = Box<dyn Fn(bool) -> Result<Option<WriteReport>, String> + Send + Sync>;
pub type Busy = Box<dyn Fn() -> bool + Send + Sync>;
pub type Notify = Box<dyn Fn(&BackupStatus) + Send + Sync>;

enum Msg {
    Touch,
    RunNow,
}

struct Shared {
    run: Run,
    busy: Busy,
    notify: Notify,
    status: Mutex<BackupStatus>,
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
            .unwrap_or_else(|_| Err("the write stopped unexpectedly".to_string()));
        let next = match ran {
            Ok(Some(report)) => BackupStatus::Ok { at, report },
            Ok(None) => before,
            Err(reason) => BackupStatus::Failed {
                at,
                reason,
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
                loop {
                    match rx.recv_timeout(TICK) {
                        Ok(Msg::Touch) => schedule.touch(Instant::now()),
                        Ok(Msg::RunNow) => asked = true,
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
                    0 => Err("the folder is gone".to_string()),
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
                _ => Err("full".to_string()),
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
    fn a_restored_status_does_not_replace_a_newer_one() {
        let old = BackupStatus::Ok {
            at: 1,
            report: WriteReport::default(),
        };
        let auto = BackupAuto::spawn(
            Box::new(|_| Err("full".to_string())),
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
