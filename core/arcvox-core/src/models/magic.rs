//! Model file format detection from the leading magic bytes.

use crate::config::Runtime;
use std::io::Read;
use std::path::Path;

/// `lmgg` is the ggml container whisper.cpp reads; `GGUF` is the llama.cpp format.
/// `None` when the file is neither, including files shorter than four bytes.
pub fn detect(path: &Path) -> Result<Option<Runtime>, std::io::Error> {
    let mut f = std::fs::File::open(path)?;
    let mut head = [0u8; 4];
    if f.read(&mut head)? < 4 {
        return Ok(None);
    }
    Ok(match &head {
        b"lmgg" => Some(Runtime::Whisper),
        b"GGUF" => Some(Runtime::Llama),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ggml_and_gguf_and_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let ggml = tmp.path().join("a.bin");
        std::fs::write(&ggml, b"lmgg\x00\x00\x00\x00rest").unwrap();
        let gguf = tmp.path().join("b.gguf");
        std::fs::write(&gguf, b"GGUF\x03\x00\x00\x00rest").unwrap();
        let other = tmp.path().join("c.txt");
        std::fs::write(&other, b"hello world").unwrap();
        assert_eq!(detect(&ggml).unwrap(), Some(Runtime::Whisper));
        assert_eq!(detect(&gguf).unwrap(), Some(Runtime::Llama));
        assert_eq!(detect(&other).unwrap(), None);
        let short = tmp.path().join("d");
        std::fs::write(&short, b"GG").unwrap();
        assert_eq!(detect(&short).unwrap(), None);
    }
}
