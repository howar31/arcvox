//! What a model file can be used for, told from its header, and which audio
//! encoder file beside it belongs to it.

use std::path::{Path, PathBuf};

use super::gguf::{self, GgufInfo};
use super::manifest::safe_segment;
use crate::config::{Capabilities, Runtime};

/// One file, as its header shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum FileKind {
    /// A whisper.cpp model: speech recognition only.
    Whisper,
    /// A llama.cpp model.
    Model(GgufInfo),
    /// A companion encoder (`mmproj`): never a model by itself.
    Encoder(GgufInfo),
}

/// `None` when the file is neither a whisper.cpp nor a GGUF file.
pub fn inspect(path: &Path) -> std::io::Result<Option<FileKind>> {
    Ok(match super::magic::detect(path)? {
        Some(Runtime::Whisper) => Some(FileKind::Whisper),
        // A header that cannot be read leaves a plain model, as before the
        // header was looked at; loading it tells the rest.
        Some(Runtime::Llama) => Some(match gguf::read(path)? {
            Some(info) if info.is_mmproj() => FileKind::Encoder(info),
            Some(info) => FileKind::Model(info),
            None => FileKind::Model(GgufInfo::default()),
        }),
        None => None,
    })
}

/// A model file found in a folder, with the audio encoder paired to it.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundModel {
    pub path: PathBuf,
    pub runtime: Runtime,
    pub capabilities: Capabilities,
    pub encoder: Option<PathBuf>,
}

impl FoundModel {
    /// A model that transcribes through an audio encoder it does not have.
    pub fn encoder_missing(&self) -> bool {
        self.runtime == Runtime::Llama && self.capabilities.transcribe && self.encoder.is_none()
    }
}

/// What a model can do: a whisper.cpp model transcribes; a llama.cpp model
/// chats unless it was trained for speech only, and transcribes when it is
/// a speech model or has an audio encoder.
pub fn capabilities(kind: &FileKind, has_encoder: bool) -> Capabilities {
    match kind {
        FileKind::Whisper => Capabilities::for_model(Runtime::Whisper, true, false),
        FileKind::Model(info) => {
            let speech_only = info.speech_only();
            Capabilities::for_model(Runtime::Llama, speech_only || has_encoder, !speech_only)
        }
        FileKind::Encoder(_) => Capabilities::default(),
    }
}

/// The model files directly inside `dir`, each paired with its audio encoder.
/// Encoders, hidden files and files of other kinds are not listed.
pub fn models_in(dir: &Path) -> Vec<FoundModel> {
    let files = classified(dir);
    let encoders: Vec<(&PathBuf, &GgufInfo)> = files
        .iter()
        .filter_map(|(p, k)| match k {
            FileKind::Encoder(i) if i.audio_encoder => Some((p, i)),
            _ => None,
        })
        .collect();
    let mut out: Vec<FoundModel> = files
        .iter()
        .filter_map(|(path, kind)| {
            let encoder = match kind {
                FileKind::Model(info) => pick_encoder(path, info, &encoders),
                FileKind::Whisper => None,
                FileKind::Encoder(_) => return None,
            };
            Some(FoundModel {
                path: path.clone(),
                runtime: if matches!(kind, FileKind::Whisper) {
                    Runtime::Whisper
                } else {
                    Runtime::Llama
                },
                capabilities: capabilities(kind, encoder.is_some()),
                encoder,
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// The audio encoder beside a model file, paired by the same rule as the listing.
pub fn encoder_beside(model: &Path) -> Option<PathBuf> {
    let dir = model.parent()?;
    models_in(dir)
        .into_iter()
        .find(|m| m.path == model)
        .and_then(|m| m.encoder)
}

/// Encoders in `dir` that no model there is paired with.
pub fn unpaired_encoders(dir: &Path) -> Vec<PathBuf> {
    let used: Vec<PathBuf> = models_in(dir)
        .into_iter()
        .filter_map(|m| m.encoder)
        .collect();
    classified(dir)
        .into_iter()
        .filter(|(p, k)| matches!(k, FileKind::Encoder(_)) && !used.contains(p))
        .map(|(p, _)| p)
        .collect()
}

fn classified(dir: &Path) -> Vec<(PathBuf, FileKind)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if !safe_segment(&name) || !path.is_file() {
                return None;
            }
            inspect(&path).ok().flatten().map(|k| (path, k))
        })
        .collect()
}

/// The encoder whose projection fits the model's width; among several, the
/// one with the closest file name. `None` when none fits or the closest are
/// tied, so a model is never paired by chance.
fn pick_encoder(
    model: &Path,
    info: &GgufInfo,
    encoders: &[(&PathBuf, &GgufInfo)],
) -> Option<PathBuf> {
    let fitting: Vec<&PathBuf> = encoders
        .iter()
        .filter(
            |(_, e)| match (info.embedding_length, e.audio_projection_dim) {
                (Some(width), Some(dim)) => width == dim,
                // A header without the widths cannot be checked; the name decides.
                _ => true,
            },
        )
        .map(|(p, _)| *p)
        .collect();
    if fitting.len() <= 1 {
        return fitting.first().map(|p| (*p).clone());
    }
    let base = stem(model);
    let score = |p: &PathBuf| common_prefix(&base, &stem(p));
    let best = fitting.iter().map(|p| score(p)).max()?;
    let mut top = fitting.iter().filter(|p| score(p) == best);
    let first = top.next()?;
    (best > 0 && top.next().is_none()).then(|| (*first).clone())
}

/// A file name reduced for comparison: lower case, no extension, no
/// `mmproj` word, letters and digits only.
fn stem(p: &Path) -> String {
    let name = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name.replace("mmproj", "")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::gguf::testing::{encoder, speech_model, text_model};

    fn put(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn a_speech_model_is_paired_with_the_encoder_of_its_width() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let small = put(d, "Qwen3-ASR-0.6B-Q8_0.gguf", &speech_model(1024));
        let large = put(d, "Qwen3-ASR-1.7B-Q8_0.gguf", &speech_model(2048));
        let small_enc = put(d, "mmproj-Qwen3-ASR-0.6B-Q8_0.gguf", &encoder(Some(1024)));
        let large_enc = put(d, "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf", &encoder(Some(2048)));
        let found = models_in(d);
        assert_eq!(found.len(), 2, "encoders are not models");
        let of = |p: &PathBuf| found.iter().find(|m| &m.path == p).unwrap().clone();
        assert_eq!(of(&small).encoder, Some(small_enc));
        assert_eq!(of(&large).encoder, Some(large_enc));
        let c = of(&small).capabilities;
        assert!(c.transcribe && !c.chat);
    }

    #[test]
    fn a_speech_model_without_an_encoder_is_listed_as_missing_it() {
        let tmp = tempfile::tempdir().unwrap();
        put(tmp.path(), "asr.gguf", &speech_model(1024));
        put(tmp.path(), "mmproj-other.gguf", &encoder(Some(4096)));
        let found = models_in(tmp.path());
        assert_eq!(found.len(), 1);
        assert!(found[0].encoder_missing());
        assert!(found[0].capabilities.transcribe && !found[0].capabilities.chat);
    }

    #[test]
    fn a_chat_model_with_an_audio_encoder_does_both() {
        let tmp = tempfile::tempdir().unwrap();
        put(tmp.path(), "omni-Q4_K_M.gguf", &text_model(2560));
        put(tmp.path(), "mmproj-omni-f16.gguf", &encoder(Some(2560)));
        let found = models_in(tmp.path());
        assert!(found[0].capabilities.transcribe && found[0].capabilities.chat);
        assert!(!found[0].encoder_missing());
    }

    #[test]
    fn a_chat_model_with_a_vision_encoder_only_chats() {
        let tmp = tempfile::tempdir().unwrap();
        put(tmp.path(), "gemma.gguf", &text_model(2560));
        put(tmp.path(), "mmproj-gemma.gguf", &encoder(None));
        let found = models_in(tmp.path());
        assert_eq!(found[0].encoder, None);
        assert!(!found[0].capabilities.transcribe && found[0].capabilities.chat);
        assert!(!found[0].encoder_missing());
    }

    #[test]
    fn one_encoder_serves_several_quantisations() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        put(d, "asr-Q8_0.gguf", &speech_model(1024));
        put(d, "asr-Q4_K_M.gguf", &speech_model(1024));
        let enc = put(d, "mmproj-asr-f16.gguf", &encoder(Some(1024)));
        assert!(models_in(d)
            .iter()
            .all(|m| m.encoder.as_ref() == Some(&enc)));
        assert!(unpaired_encoders(d).is_empty());
    }

    #[test]
    fn tied_encoders_pair_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        put(d, "model.gguf", &speech_model(1024));
        put(d, "mmproj-a.gguf", &encoder(Some(1024)));
        put(d, "mmproj-b.gguf", &encoder(Some(1024)));
        assert!(models_in(d)[0].encoder_missing());
        assert_eq!(unpaired_encoders(d).len(), 2);
    }

    #[test]
    fn whisper_files_transcribe_and_others_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let w = put(tmp.path(), "ggml-x.bin", b"lmgg0000");
        put(tmp.path(), "notes.txt", b"hello world");
        put(tmp.path(), ".hidden.gguf", &text_model(8));
        let found = models_in(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, w);
        assert_eq!(found[0].runtime, Runtime::Whisper);
        assert!(found[0].capabilities.transcribe && found[0].capabilities.language_hint);
    }

    #[test]
    fn encoder_beside_follows_the_listing() {
        let tmp = tempfile::tempdir().unwrap();
        let m = put(tmp.path(), "asr.gguf", &speech_model(1024));
        let e = put(tmp.path(), "mmproj-asr.gguf", &encoder(Some(1024)));
        assert_eq!(encoder_beside(&m), Some(e));
    }
}
