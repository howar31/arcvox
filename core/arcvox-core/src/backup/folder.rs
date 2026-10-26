//! Whether a backup folder can be written to right now.

use std::path::Path;

use serde::Serialize;

/// What a look at the backup folder found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderState {
    Usable,
    /// Not there: removed, moved, or on a volume that is not connected.
    Missing,
    /// There, but a file cannot be made in it.
    NotWritable,
    /// There, but the system keeps this app out of it.
    Blocked,
    /// The suggested folder before any write went to it: not looked at,
    /// since it need not exist yet and the first write makes it.
    Unmade,
}

impl FolderState {
    pub fn usable(self) -> bool {
        self == FolderState::Usable
    }
}

/// The system's answer when its privacy rules refuse a call, as opposed to
/// the folder's own permissions.
#[cfg(unix)]
const NOT_PERMITTED: i32 = 1;

fn refused(e: &std::io::Error) -> FolderState {
    #[cfg(unix)]
    if e.raw_os_error() == Some(NOT_PERMITTED) {
        return FolderState::Blocked;
    }
    let _ = e;
    FolderState::NotWritable
}

/// Looks at `dir`. With `probe` a small file is made in it and removed
/// again, which is the only reliable answer to "can it be written"; without,
/// only what the folder says about itself is read.
pub fn folder_state(dir: &Path, probe: bool) -> FolderState {
    let meta = match std::fs::metadata(dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return refused(&e),
        Err(_) => return FolderState::Missing,
    };
    if !meta.is_dir() {
        return FolderState::Missing;
    }
    if !probe {
        return if meta.permissions().readonly() {
            FolderState::NotWritable
        } else {
            FolderState::Usable
        };
    }
    let file = dir.join(format!(".write-test-{}", std::process::id()));
    let made = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file);
    match made {
        Ok(_) => {
            let _ = std::fs::remove_file(&file);
            FolderState::Usable
        }
        // A test file left by a run that was cut short.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            match std::fs::remove_file(&file) {
                Ok(()) => FolderState::Usable,
                Err(e) => refused(&e),
            }
        }
        // The folder went away between the two calls.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FolderState::Missing,
        Err(e) => refused(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_that_takes_a_file_is_usable_and_keeps_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(folder_state(tmp.path(), true), FolderState::Usable);
        assert_eq!(folder_state(tmp.path(), false), FolderState::Usable);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_folder_that_is_not_there_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let gone = tmp.path().join("volume").join("sync");
        assert_eq!(folder_state(&gone, true), FolderState::Missing);
        assert_eq!(folder_state(&gone, false), FolderState::Missing);
        assert!(!tmp.path().join("volume").exists());
    }

    #[test]
    fn a_file_in_the_folders_place_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("sync");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(folder_state(&file, true), FolderState::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_without_write_permission_is_not_writable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sync");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let probed = folder_state(&dir, true);
        let read = folder_state(&dir, false);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(probed, FolderState::NotWritable);
        assert_eq!(read, FolderState::NotWritable);
    }

    #[cfg(unix)]
    #[test]
    fn a_call_the_system_refuses_is_blocked() {
        let e = std::io::Error::from_raw_os_error(NOT_PERMITTED);
        assert_eq!(refused(&e), FolderState::Blocked);
        let e = std::io::Error::from_raw_os_error(13);
        assert_eq!(refused(&e), FolderState::NotWritable);
    }

    #[test]
    fn a_test_file_left_behind_does_not_count_against_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let left = tmp
            .path()
            .join(format!(".write-test-{}", std::process::id()));
        std::fs::write(&left, b"").unwrap();
        assert_eq!(folder_state(tmp.path(), true), FolderState::Usable);
        assert!(!left.exists());
    }
}
