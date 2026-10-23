//! The statistics share card: the page renders the PNG; these commands put
//! its bytes on the clipboard or into a file.

use std::path::{Path, PathBuf};

use tauri::ipc::{InvokeBody, Request};
use tauri::State;

use super::{CmdError, CmdResult};
use crate::app::AppState;

/// The header that names the file to write.
const PATH_HEADER: &str = "x-path";

fn png_body<'a>(request: &'a Request<'_>) -> CmdResult<&'a [u8]> {
    match request.body() {
        InvokeBody::Raw(bytes) if !bytes.is_empty() => Ok(bytes),
        _ => Err(CmdError::new("bad_request", "the image bytes are missing")),
    }
}

/// Puts the PNG on the clipboard.
#[tauri::command]
pub fn share_copy_image(state: State<'_, AppState>, request: Request<'_>) -> CmdResult<()> {
    let png = png_body(&request)?;
    if !state.platform.clipboard_write_png(png) {
        return Err(CmdError::new(
            "clipboard_failed",
            "the clipboard refused the image",
        ));
    }
    Ok(())
}

/// Writes the PNG to the file named in the `x-path` header (percent-encoded
/// UTF-8). Returns the path written, with `.png` appended when the name had
/// no such extension.
#[tauri::command]
pub fn share_save_image(request: Request<'_>) -> CmdResult<String> {
    let encoded = request
        .headers()
        .get(PATH_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| CmdError::new("bad_request", "the file path is missing"))?;
    let path = percent_decode(encoded);
    let png = png_body(&request)?;
    Ok(save_png(&path, png)?.to_string_lossy().into_owned())
}

/// Writes `png` to `path`, with `.png` appended when the name has no such
/// extension; an existing file is replaced (the save dialog asked).
pub fn save_png(path: &str, png: &[u8]) -> CmdResult<PathBuf> {
    if path.trim().is_empty() {
        return Err(CmdError::new("bad_request", "the file path is empty"));
    }
    let mut out = PathBuf::from(path);
    let has_png = out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    if !has_png {
        let name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.set_file_name(format!("{name}.png"));
    }
    if out.is_dir() {
        return Err(CmdError::new("bad_request", "the path is a folder"));
    }
    std::fs::write(&out, png).map_err(|e| {
        CmdError::new(
            "write_failed",
            format!("{}: {e}", Path::new(&out).display()),
        )
    })?;
    Ok(out)
}

/// Decodes `%XX` escapes of a UTF-8 string; a bad escape is kept as is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_gets_the_png_extension_and_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("Arcvox 統計 2026-10-23");
        let written = save_png(bare.to_str().unwrap(), b"PNG1").unwrap();
        assert_eq!(written, dir.path().join("Arcvox 統計 2026-10-23.png"));
        assert_eq!(std::fs::read(&written).unwrap(), b"PNG1");
        let again = save_png(written.to_str().unwrap(), b"PNG2").unwrap();
        assert_eq!(again, written);
        assert_eq!(std::fs::read(&written).unwrap(), b"PNG2");
        let upper = save_png(dir.path().join("x.PNG").to_str().unwrap(), b"P").unwrap();
        assert_eq!(upper.file_name().unwrap(), "x.PNG");
    }

    #[test]
    fn an_empty_path_and_a_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(save_png("  ", b"P").unwrap_err().code, Some("bad_request"));
        let folder = dir.path().join("f.png");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(
            save_png(folder.to_str().unwrap(), b"P").unwrap_err().code,
            Some("bad_request")
        );
    }

    #[test]
    fn the_path_header_is_percent_decoded() {
        assert_eq!(
            percent_decode("%2Ftmp%2Fx%2FArcvox%20%E7%B5%B1%E8%A8%88.png"),
            "/tmp/x/Arcvox 統計.png"
        );
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("%2"), "%2");
    }
}
