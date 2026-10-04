//! The identity of this installation: a random id that history rows and
//! statistics carry, and a name the user can change.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const FILE_NAME: &str = "device.json";
/// Longest device name, in characters.
pub const NAME_MAX: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    id: String,
    name: String,
    /// A digest of the id and the machine it was made on; empty when the
    /// machine gave no id. Tells a copied data directory from the original.
    #[serde(default)]
    bound_to: String,
}

fn binding(id: &str, hardware: &str) -> String {
    let mut h = Sha256::new();
    h.update(id.as_bytes());
    h.update(b":");
    h.update(hardware.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn clean_name(name: &str) -> Option<String> {
    let name: String = name.trim().chars().take(NAME_MAX).collect();
    (!name.is_empty()).then_some(name)
}

fn read(dir: &Path) -> Option<Stored> {
    let text = std::fs::read_to_string(dir.join(FILE_NAME)).ok()?;
    let stored: Stored = serde_json::from_str(&text).ok()?;
    (!stored.id.is_empty()).then_some(stored)
}

fn write(dir: &Path, stored: &Stored) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&serde_json::to_vec_pretty(stored).map_err(std::io::Error::other)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(dir.join(FILE_NAME)).map_err(|e| e.error)?;
    Ok(())
}

impl Device {
    /// Reads `device.json` in `dir`, or creates it. `hardware` is the
    /// machine's own id; when the stored identity was made on another
    /// machine, a new id is made and the name is kept.
    pub fn load_or_create(
        dir: &Path,
        hardware: Option<&str>,
        default_name: &str,
    ) -> std::io::Result<Device> {
        let fresh = |name: String| {
            let id = uuid::Uuid::new_v4().to_string();
            let bound_to = hardware.map(|h| binding(&id, h)).unwrap_or_default();
            Stored { id, name, bound_to }
        };
        let stored = match (read(dir), hardware) {
            (Some(s), None) => return Ok(s.into()),
            (Some(s), Some(h)) if s.bound_to == binding(&s.id, h) => return Ok(s.into()),
            (Some(mut s), Some(h)) if s.bound_to.is_empty() => {
                s.bound_to = binding(&s.id, h);
                s
            }
            (Some(s), Some(_)) => fresh(s.name),
            (None, _) => fresh(clean_name(default_name).unwrap_or_else(|| "Device".into())),
        };
        write(dir, &stored)?;
        Ok(stored.into())
    }

    /// A new id under the name already stored: for a data directory whose
    /// database is new, so that rows of the earlier database (in a backup)
    /// and rows of this one never share ids.
    pub fn renew(
        dir: &Path,
        hardware: Option<&str>,
        default_name: &str,
    ) -> std::io::Result<Device> {
        let Some(old) = read(dir) else {
            return Self::load_or_create(dir, hardware, default_name);
        };
        let id = uuid::Uuid::new_v4().to_string();
        let stored = Stored {
            bound_to: hardware.map(|h| binding(&id, h)).unwrap_or_default(),
            id,
            name: old.name,
        };
        write(dir, &stored)?;
        Ok(stored.into())
    }

    /// Gives this installation another name. The id stays.
    pub fn rename(dir: &Path, name: &str) -> std::io::Result<Device> {
        let name = clean_name(name).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "the name is empty")
        })?;
        let mut stored = read(dir).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no device identity")
        })?;
        stored.name = name;
        write(dir, &stored)?;
        Ok(stored.into())
    }
}

impl From<Stored> for Device {
    fn from(s: Stored) -> Self {
        Device {
            id: s.id,
            name: s.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_new_directory_gets_an_id_and_keeps_it() {
        let d = dir();
        let a = Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap();
        let b = Device::load_or_create(d.path(), Some("HW-1"), "Other").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.name, "Mac");
        assert_eq!(a.id.len(), 36);
        let raw = std::fs::read_to_string(d.path().join(FILE_NAME)).unwrap();
        assert!(!raw.contains("HW-1"));
    }

    #[test]
    fn other_hardware_gets_a_new_id_and_keeps_the_name() {
        let d = dir();
        let a = Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap();
        let b = Device::load_or_create(d.path(), Some("HW-2"), "Ignored").unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(b.name, "Mac");
        // The new id is bound to the new hardware.
        let c = Device::load_or_create(d.path(), Some("HW-2"), "Ignored").unwrap();
        assert_eq!(b, c);
    }

    #[test]
    fn no_hardware_id_keeps_the_identity() {
        let d = dir();
        let a = Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap();
        assert_eq!(Device::load_or_create(d.path(), None, "Mac").unwrap(), a);

        let d = dir();
        let a = Device::load_or_create(d.path(), None, "Mac").unwrap();
        let b = Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap();
        assert_eq!(a, b);
        let c = Device::load_or_create(d.path(), Some("HW-2"), "Mac").unwrap();
        assert_ne!(a.id, c.id);
    }

    #[test]
    fn a_damaged_file_is_replaced() {
        let d = dir();
        std::fs::write(d.path().join(FILE_NAME), "{").unwrap();
        let a = Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap();
        assert_eq!(a.id.len(), 36);
        assert_eq!(
            Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap(),
            a
        );
    }

    #[test]
    fn a_renewed_identity_has_a_new_id_and_the_same_name() {
        let d = dir();
        let a = Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap();
        let b = Device::renew(d.path(), Some("HW-1"), "Other").unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(b.name, "Mac");
        assert_eq!(
            Device::load_or_create(d.path(), Some("HW-1"), "x").unwrap(),
            b
        );
        // Without a stored identity it is the same as creating one.
        let d = dir();
        assert_eq!(
            Device::renew(d.path(), None, "Fresh").unwrap().name,
            "Fresh"
        );
    }

    #[test]
    fn rename_keeps_the_id() {
        let d = dir();
        let a = Device::load_or_create(d.path(), Some("HW-1"), "Mac").unwrap();
        let b = Device::rename(d.path(), "  Desk  ").unwrap();
        assert_eq!(b.id, a.id);
        assert_eq!(b.name, "Desk");
        assert_eq!(
            Device::load_or_create(d.path(), Some("HW-1"), "x").unwrap(),
            b
        );
    }

    #[test]
    fn an_empty_name_is_refused_and_a_long_one_is_cut() {
        let d = dir();
        Device::load_or_create(d.path(), None, "Mac").unwrap();
        let e = Device::rename(d.path(), "   ").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        let long = "名".repeat(200);
        assert_eq!(
            Device::rename(d.path(), &long)
                .unwrap()
                .name
                .chars()
                .count(),
            80
        );
    }
}
