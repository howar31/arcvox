//! The few GGUF header values that tell what a model file is for, read
//! without loading the weights.

use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

/// Tag a speech recognition model carries in `general.tags`.
const ASR_TAG: &str = "automatic-speech-recognition";
/// Longest key or kept string value read; a longer one marks a broken header.
const MAX_STRING: u64 = 1 << 20;
/// More key-value pairs than any real model carries; a larger count marks a broken header.
const MAX_KV: u64 = 1 << 16;

/// What a GGUF file's header says about its use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GgufInfo {
    /// `general.type`: `model`, or `mmproj` for a companion encoder.
    pub kind: Option<String>,
    pub architecture: Option<String>,
    /// `general.size_label`, such as `8B`.
    pub size_label: Option<String>,
    /// `general.license`.
    pub license: Option<String>,
    /// `general.file_type`: the quantisation, as llama.cpp numbers it.
    pub file_type: Option<u64>,
    pub tags: Vec<String>,
    /// `<architecture>.embedding_length`: the width an encoder must project to.
    pub embedding_length: Option<u64>,
    /// `clip.has_audio_encoder`.
    pub audio_encoder: bool,
    /// `clip.audio.projection_dim`: the model width the audio encoder feeds.
    pub audio_projection_dim: Option<u64>,
}

impl GgufInfo {
    pub fn is_mmproj(&self) -> bool {
        self.kind.as_deref() == Some("mmproj")
    }

    /// A model trained for speech recognition only: it transcribes and does not chat.
    pub fn speech_only(&self) -> bool {
        self.tags.iter().any(|t| t == ASR_TAG)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Uint(u64),
    Bool(bool),
    Str(String),
    Strs(Vec<String>),
    Other,
}

/// Reads the header of a GGUF file; `None` when the file is not one or its
/// header cannot be read.
pub fn read(path: &Path) -> std::io::Result<Option<GgufInfo>> {
    let mut r = BufReader::new(std::fs::File::open(path)?);
    let mut magic = [0u8; 4];
    if r.read_exact(&mut magic).is_err() || &magic != b"GGUF" {
        return Ok(None);
    }
    Ok(parse(&mut r).ok())
}

fn parse<R: Read + Seek>(r: &mut R) -> std::io::Result<GgufInfo> {
    let version = u32_le(r)?;
    if version < 2 {
        return Err(broken("GGUF version 1 is not read"));
    }
    let _tensors = u64_le(r)?;
    let kv = u64_le(r)?;
    if kv > MAX_KV {
        return Err(broken("too many header entries"));
    }
    let mut values: HashMap<String, Value> = HashMap::new();
    for _ in 0..kv {
        let key = string(r)?;
        let ty = u32_le(r)?;
        let keep = key.starts_with("general.")
            || key.starts_with("clip.")
            || key.ends_with(".embedding_length");
        let v = value(r, ty, keep)?;
        if keep {
            values.insert(key, v);
        }
    }
    let text = |k: &str| match values.get(k) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let uint = |k: &str| match values.get(k) {
        Some(Value::Uint(n)) => Some(*n),
        _ => None,
    };
    let architecture = text("general.architecture");
    Ok(GgufInfo {
        kind: text("general.type"),
        file_type: uint("general.file_type"),
        size_label: text("general.size_label"),
        license: text("general.license"),
        embedding_length: architecture
            .as_deref()
            .and_then(|a| uint(&format!("{a}.embedding_length"))),
        architecture,
        tags: match values.get("general.tags") {
            Some(Value::Strs(t)) => t.clone(),
            _ => Vec::new(),
        },
        audio_encoder: matches!(
            values.get("clip.has_audio_encoder"),
            Some(Value::Bool(true))
        ),
        audio_projection_dim: uint("clip.audio.projection_dim"),
    })
}

fn broken(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, what)
}

fn u32_le<R: Read>(r: &mut R) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn u64_le<R: Read>(r: &mut R) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn string<R: Read>(r: &mut R) -> std::io::Result<String> {
    let len = u64_le(r)?;
    if len > MAX_STRING {
        return Err(broken("string too long"));
    }
    let mut b = vec![0u8; len as usize];
    r.read_exact(&mut b)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

fn skip_string<R: Read + Seek>(r: &mut R) -> std::io::Result<()> {
    let len = u64_le(r)?;
    let len = i64::try_from(len).map_err(|_| broken("string too long"))?;
    r.seek_relative(len)
}

/// Bytes of a fixed-size value type; `None` for strings and arrays.
fn fixed_size(ty: u32) -> std::io::Result<Option<i64>> {
    Ok(match ty {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        8 | 9 => None,
        _ => return Err(broken("unknown value type")),
    })
}

/// Reads one value; only what `keep` asks for is decoded, the rest is skipped.
fn value<R: Read + Seek>(r: &mut R, ty: u32, keep: bool) -> std::io::Result<Value> {
    match ty {
        // uint32, uint64
        4 if keep => Ok(Value::Uint(u64::from(u32_le(r)?))),
        10 if keep => Ok(Value::Uint(u64_le(r)?)),
        7 if keep => {
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            Ok(Value::Bool(b[0] != 0))
        }
        8 if keep => Ok(Value::Str(string(r)?)),
        8 => skip_string(r).map(|_| Value::Other),
        9 => {
            let item = u32_le(r)?;
            let n = u64_le(r)?;
            match fixed_size(item)? {
                Some(size) => {
                    let bytes = i64::try_from(n)
                        .ok()
                        .and_then(|n| n.checked_mul(size))
                        .ok_or_else(|| broken("array too long"))?;
                    r.seek_relative(bytes)?;
                    Ok(Value::Other)
                }
                None if item == 8 && keep && n <= MAX_KV => {
                    let mut out = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        out.push(string(r)?);
                    }
                    Ok(Value::Strs(out))
                }
                None if item == 8 => {
                    for _ in 0..n {
                        skip_string(r)?;
                    }
                    Ok(Value::Other)
                }
                None => Err(broken("nested arrays are not read")),
            }
        }
        _ => {
            let size = fixed_size(ty)?.expect("strings and arrays are handled above");
            r.seek_relative(size)?;
            Ok(Value::Other)
        }
    }
}

/// GGUF headers for tests: model, encoder and text model shapes.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {

    /// Writes a GGUF header with the given string, string-array, uint32 and bool entries.
    #[derive(Default)]
    pub struct Header(Vec<u8>, u64);

    impl Header {
        fn key(mut self, k: &str, ty: u32) -> Self {
            self.0.extend((k.len() as u64).to_le_bytes());
            self.0.extend(k.as_bytes());
            self.0.extend(ty.to_le_bytes());
            self.1 += 1;
            self
        }
        pub fn str(self, k: &str, v: &str) -> Self {
            let mut h = self.key(k, 8);
            h.0.extend((v.len() as u64).to_le_bytes());
            h.0.extend(v.as_bytes());
            h
        }
        pub fn strs(self, k: &str, v: &[&str]) -> Self {
            let mut h = self.key(k, 9);
            h.0.extend(8u32.to_le_bytes());
            h.0.extend((v.len() as u64).to_le_bytes());
            for s in v {
                h.0.extend((s.len() as u64).to_le_bytes());
                h.0.extend(s.as_bytes());
            }
            h
        }
        pub fn u32(self, k: &str, v: u32) -> Self {
            let mut h = self.key(k, 4);
            h.0.extend(v.to_le_bytes());
            h
        }
        pub fn bool(self, k: &str, v: bool) -> Self {
            let mut h = self.key(k, 7);
            h.0.push(u8::from(v));
            h
        }
        pub fn floats(self, k: &str, n: u64) -> Self {
            let mut h = self.key(k, 9);
            h.0.extend(6u32.to_le_bytes());
            h.0.extend(n.to_le_bytes());
            h.0.extend(std::iter::repeat_n(0u8, 4 * n as usize));
            h
        }
        pub fn bytes(&self) -> Vec<u8> {
            let mut out = b"GGUF".to_vec();
            out.extend(3u32.to_le_bytes());
            out.extend(0u64.to_le_bytes());
            out.extend(self.1.to_le_bytes());
            out.extend(&self.0);
            out.extend(b"tensor data");
            out
        }
    }

    /// The header of a speech recognition model file (Qwen3-ASR shape).
    pub fn speech_model(width: u32) -> Vec<u8> {
        Header::default()
            .str("general.architecture", "qwen3vl")
            .str("general.type", "model")
            .strs("general.tags", &["automatic-speech-recognition"])
            .str("general.size_label", "752M")
            .str("general.license", "apache-2.0")
            .u32("general.file_type", 7)
            .u32("qwen3vl.embedding_length", width)
            .strs("tokenizer.ggml.tokens", &["a", "b", "c"])
            .floats("tokenizer.ggml.scores", 3)
            .bytes()
    }

    /// The header of an encoder file; `audio_width` gives it an audio encoder.
    pub fn encoder(audio_width: Option<u32>) -> Vec<u8> {
        let h = Header::default()
            .str("general.architecture", "clip")
            .str("general.type", "mmproj");
        match audio_width {
            Some(w) => h
                .bool("clip.has_audio_encoder", true)
                .u32("clip.audio.projection_dim", w)
                .bytes(),
            None => h.bool("clip.has_vision_encoder", true).bytes(),
        }
    }

    /// The header of a text model file.
    pub fn text_model(width: u32) -> Vec<u8> {
        Header::default()
            .str("general.architecture", "gemma3")
            .str("general.type", "model")
            .strs("general.tags", &["image-text-to-text"])
            .u32("gemma3.embedding_length", width)
            .bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn info(bytes: &[u8]) -> Option<GgufInfo> {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("m.gguf");
        std::fs::write(&p, bytes).unwrap();
        read(&p).unwrap()
    }

    #[test]
    fn reads_a_speech_model() {
        let i = info(&speech_model(1024)).unwrap();
        assert_eq!(i.architecture.as_deref(), Some("qwen3vl"));
        assert_eq!(i.embedding_length, Some(1024));
        assert!(i.speech_only() && !i.is_mmproj() && !i.audio_encoder);
        assert_eq!(i.size_label.as_deref(), Some("752M"));
        assert_eq!(i.license.as_deref(), Some("apache-2.0"));
        assert_eq!(i.file_type, Some(7));
    }

    #[test]
    fn reads_an_audio_encoder_and_a_vision_one() {
        let a = info(&encoder(Some(2048))).unwrap();
        assert!(a.is_mmproj() && a.audio_encoder);
        assert_eq!(a.audio_projection_dim, Some(2048));
        let v = info(&encoder(None)).unwrap();
        assert!(v.is_mmproj() && !v.audio_encoder);
    }

    #[test]
    fn reads_a_text_model() {
        let i = info(&text_model(2560)).unwrap();
        assert!(!i.speech_only() && !i.is_mmproj());
        assert_eq!(i.embedding_length, Some(2560));
    }

    #[test]
    fn other_files_and_broken_headers_are_none() {
        assert_eq!(info(b"lmgg0000"), None);
        assert_eq!(info(b"GGUF"), None);
        let mut cut = speech_model(1024);
        cut.truncate(40);
        assert_eq!(info(&cut), None);
        let mut huge = b"GGUF".to_vec();
        huge.extend(3u32.to_le_bytes());
        huge.extend(0u64.to_le_bytes());
        huge.extend(u64::MAX.to_le_bytes());
        assert_eq!(info(&huge), None);
    }
}
