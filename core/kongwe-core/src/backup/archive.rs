//! A backup tree as one file: a zip archive of the same paths.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::format::*;

fn zip_err(path: &Path, e: zip::result::ZipError) -> BackupError {
    match e {
        zip::result::ZipError::Io(source) => io_err(path, source),
        other => BackupError::NotABackup(other.to_string()),
    }
}

fn add_dir(
    zip: &mut ZipWriter<BufWriter<File>>,
    root: &Path,
    dir: &Path,
    out: &Path,
) -> Result<(), BackupError> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| io_err(dir, e))?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            add_dir(zip, root, &path, out)?;
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let name: Vec<_> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect();
        // Recordings do not get smaller; the text does.
        let method = if path.extension().is_some_and(|x| x == "wav") {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .large_file(true);
        zip.start_file(name.join("/"), options)
            .map_err(|e| zip_err(out, e))?;
        let mut file = File::open(&path).map_err(|e| io_err(&path, e))?;
        std::io::copy(&mut file, zip).map_err(|e| io_err(out, e))?;
    }
    Ok(())
}

/// Packs the tree at `tree` into the file at `file`. The file appears under
/// its name only when it is complete.
pub fn pack(tree: &Path, file: &Path) -> Result<(), BackupError> {
    let mut part = file.as_os_str().to_owned();
    part.push(".part");
    let part = Path::new(&part);
    let packed = (|| {
        let out = File::create(part).map_err(|e| io_err(part, e))?;
        let mut zip = ZipWriter::new(BufWriter::new(out));
        add_dir(&mut zip, tree, tree, part)?;
        zip.finish().map_err(|e| zip_err(part, e))?;
        std::fs::rename(part, file).map_err(|e| io_err(file, e))
    })();
    if packed.is_err() {
        let _ = std::fs::remove_file(part);
    }
    packed
}

/// Unpacks the archive at `file` into the directory `into`. An archive
/// with an entry that would land outside `into` is refused as a whole.
pub fn unpack(file: &Path, into: &Path) -> Result<(), BackupError> {
    let reader = File::open(file).map_err(|e| io_err(file, e))?;
    let mut zip = ZipArchive::new(BufReader::new(reader)).map_err(|e| zip_err(file, e))?;
    let mut names = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| zip_err(file, e))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(BackupError::NotABackup(format!(
                "the entry `{}` leaves the archive",
                entry.name()
            )));
        };
        names.push((rel, entry.is_dir()));
    }
    for (i, (rel, is_dir)) in names.into_iter().enumerate() {
        let target = into.join(rel);
        if is_dir {
            std::fs::create_dir_all(&target).map_err(|e| io_err(&target, e))?;
            continue;
        }
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
        }
        let mut entry = zip.by_index(i).map_err(|e| zip_err(file, e))?;
        let mut out = File::create(&target).map_err(|e| io_err(&target, e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| io_err(&target, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_packed_tree_unpacks_to_the_same_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tree = tmp.path().join("tree");
        std::fs::create_dir_all(tree.join("devices/d1/recordings/2026-10")).unwrap();
        std::fs::write(
            tree.join(MANIFEST_NAME),
            b"{\"format\":\"kongwe-backup\",\"version\":1}",
        )
        .unwrap();
        std::fs::write(tree.join("devices/d1/device.json"), "{\"名\":1}").unwrap();
        std::fs::write(
            tree.join("devices/d1/recordings/2026-10/1.wav"),
            vec![7u8; 5000],
        )
        .unwrap();
        let file = tmp.path().join("b.kongwe");
        pack(&tree, &file).unwrap();
        assert!(file.is_file());
        assert!(!tmp.path().join("b.kongwe.part").exists());

        let out = tmp.path().join("out");
        unpack(&file, &out).unwrap();
        for rel in [
            MANIFEST_NAME,
            "devices/d1/device.json",
            "devices/d1/recordings/2026-10/1.wav",
        ] {
            assert_eq!(
                std::fs::read(out.join(rel)).unwrap(),
                std::fs::read(tree.join(rel)).unwrap(),
                "{rel}"
            );
        }
    }

    #[test]
    fn entries_that_leave_the_archive_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("evil.kongwe");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST_NAME, options).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.start_file("../outside.txt", options).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();

        let out = tmp.path().join("deep/out");
        let got = unpack(&file, &out);
        assert!(matches!(got, Err(BackupError::NotABackup(_))), "{got:?}");
        assert!(!tmp.path().join("deep/outside.txt").exists());
    }

    #[test]
    fn a_file_that_is_not_an_archive_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("plain.kongwe");
        std::fs::write(&file, b"{\"format\":\"kongwe-settings\"}").unwrap();
        let got = unpack(&file, &tmp.path().join("out"));
        assert!(matches!(got, Err(BackupError::NotABackup(_))), "{got:?}");
        let missing = unpack(&tmp.path().join("none.kongwe"), &tmp.path().join("out2"));
        assert!(matches!(missing, Err(BackupError::Io { .. })));
    }
}
