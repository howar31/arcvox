use super::ModelError;
use crate::config::{Capabilities, Runtime};
use serde::{Deserialize, Serialize};

/// One path component that cannot leave its parent directory. Hidden names are
/// refused too, so nothing collides with the store's temporary files.
pub(crate) fn safe_segment(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && !s.contains(['/', '\\', '\0'])
}

/// Highest manifest `schema_version` this build understands.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Asr,
    Llm,
}

/// Unknown fields are ignored so older builds keep reading newer catalogues.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub runtime: Runtime,
    pub role: ModelRole,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub languages: Vec<String>,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub license: String,
    #[serde(default)]
    pub recommended: bool,
    #[serde(default)]
    pub min_ram_gb: u32,
    /// The audio encoder a llama.cpp speech model reads the sound through,
    /// kept beside the model file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mmproj: Option<ModelFile>,
}

/// A second file of a catalogue model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelFile {
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
}

impl ModelFile {
    pub fn file_name(&self) -> String {
        url_file_name(&self.url).unwrap_or_default()
    }
}

/// Last path segment of a download URL, `None` when there is none.
fn url_file_name(url: &str) -> Option<String> {
    url.split(['?', '#'])
        .next()
        .and_then(|u| u.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl ModelEntry {
    /// What the model is used for, from its role.
    pub fn capabilities(&self) -> Capabilities {
        let speech = self.role == ModelRole::Asr;
        Capabilities::for_model(self.runtime, speech, !speech)
    }

    /// Bytes of every file the model needs.
    pub fn total_bytes(&self) -> u64 {
        self.size_bytes + self.mmproj.as_ref().map_or(0, |m| m.size_bytes)
    }

    /// Last path segment of the download URL; `<id>.bin` when there is none.
    pub fn file_name(&self) -> String {
        url_file_name(&self.url).unwrap_or_else(|| format!("{}.bin", self.id))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub models: Vec<ModelEntry>,
}

impl Manifest {
    /// Entries this build cannot read (a newer runtime or field shape) are
    /// left out, so one new entry never hides the rest of the catalogue.
    pub fn parse(json: &str) -> Result<Manifest, ModelError> {
        #[derive(Deserialize)]
        struct Raw {
            schema_version: u32,
            models: Vec<serde_json::Value>,
        }
        let raw: Raw =
            serde_json::from_str(json).map_err(|e| ModelError::Manifest(e.to_string()))?;
        if raw.schema_version > MANIFEST_SCHEMA_VERSION {
            return Err(ModelError::Manifest(format!(
                "schema version {} is newer than supported version {}",
                raw.schema_version, MANIFEST_SCHEMA_VERSION
            )));
        }
        let mut models = Vec::with_capacity(raw.models.len());
        for v in raw.models {
            let e: ModelEntry = match serde_json::from_value(v) {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("manifest entry skipped: {err}");
                    continue;
                }
            };
            let encoder_ok = match &e.mmproj {
                Some(m) => safe_segment(&m.file_name()) && m.file_name() != e.file_name(),
                // A llama.cpp speech model hears through its encoder.
                None => !(e.runtime == Runtime::Llama && e.role == ModelRole::Asr),
            };
            if !safe_segment(&e.id)
                || e.id == "custom"
                || !safe_segment(&e.file_name())
                || !encoder_ok
            {
                return Err(ModelError::Manifest(format!(
                    "model `{}` has an unsafe id or file name",
                    e.id
                )));
            }
            models.push(e);
        }
        Ok(Manifest {
            schema_version: raw.schema_version,
            models,
        })
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn recommended(&self, role: ModelRole) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.recommended && m.role == role)
    }
}

/// Where the manifest in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestSource {
    Remote,
    Cached,
    Bundled,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SAMPLE: &str = r#"{"schema_version":1,"models":[
      {"id":"a","runtime":"whisper","role":"asr","label":"A","description":"","languages":["zh-TW"],"url":"https://h/x/ggml-a.bin","sha256":"00","size_bytes":10,"license":"Apache-2.0","recommended":true,"min_ram_gb":8},
      {"id":"b","runtime":"llama","role":"llm","label":"B","description":"","languages":[],"url":"https://h/y/b.gguf","sha256":"11","size_bytes":20,"license":"MIT","recommended":false,"min_ram_gb":8}]}"#;

    #[test]
    fn parses_and_finds_recommended() {
        let m = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(m.get("b").unwrap().runtime, Runtime::Llama);
        assert_eq!(m.recommended(ModelRole::Asr).unwrap().id, "a");
        assert!(m.recommended(ModelRole::Llm).is_none());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"schema_version":1,"published":"later","models":[
          {"id":"a","runtime":"whisper","role":"asr","label":"A","url":"https://h/a.bin","sha256":"00","size_bytes":1,"license":"MIT","alternates":["https://m/a.bin"]}]}"#;
        let m = Manifest::parse(json).unwrap();
        assert_eq!(m.get("a").unwrap().file_name(), "a.bin");
    }

    #[test]
    fn entries_this_build_cannot_read_are_left_out() {
        let json = r#"{"schema_version":1,"models":[
          {"id":"a","runtime":"whisper","role":"asr","label":"A","url":"https://h/a.bin","sha256":"00","size_bytes":1,"license":"MIT"},
          {"id":"n","runtime":"onnx","role":"asr","label":"N","url":"https://h/n.onnx","sha256":"00","size_bytes":1,"license":"MIT"}]}"#;
        let m = Manifest::parse(json).unwrap();
        assert_eq!(m.models.len(), 1);
        assert!(m.get("a").is_some());
    }

    #[test]
    fn a_speech_model_on_llama_carries_its_encoder() {
        let entry = |mmproj: &str| {
            format!(
                r#"{{"schema_version":1,"models":[{{"id":"q","runtime":"llama","role":"asr","label":"Q","url":"https://h/q.gguf","sha256":"0","size_bytes":10,"license":"Apache-2.0"{mmproj}}}]}}"#
            )
        };
        let m = Manifest::parse(&entry(
            r#","mmproj":{"url":"https://h/mmproj-q.gguf","sha256":"1","size_bytes":3}"#,
        ))
        .unwrap();
        let q = m.get("q").unwrap();
        assert_eq!(q.mmproj.as_ref().unwrap().file_name(), "mmproj-q.gguf");
        assert_eq!(q.total_bytes(), 13);
        let c = q.capabilities();
        assert!(c.transcribe && !c.chat && !c.language_hint && c.hotwords);
        assert!(Manifest::parse(&entry("")).is_err(), "no encoder");
        assert!(
            Manifest::parse(&entry(
                r#","mmproj":{"url":"https://h/q.gguf","sha256":"1","size_bytes":3}"#
            ))
            .is_err(),
            "same file name"
        );
        assert!(
            Manifest::parse(&entry(
                r#","mmproj":{"url":"https://h/x/..","sha256":"1","size_bytes":3}"#
            ))
            .is_err(),
            "unsafe file name"
        );
    }

    #[test]
    fn newer_schema_is_rejected() {
        assert!(matches!(
            Manifest::parse(r#"{"schema_version":2,"models":[]}"#),
            Err(ModelError::Manifest(_))
        ));
    }

    #[test]
    fn unsafe_ids_and_file_names_are_rejected() {
        let entry = |id: &str, url: &str| {
            format!(
                r#"{{"schema_version":1,"models":[{{"id":"{id}","runtime":"llama","role":"llm","label":"L","url":"{url}","sha256":"0","size_bytes":1,"license":"MIT"}}]}}"#
            )
        };
        assert!(Manifest::parse(&entry("ok", "https://h/ok.gguf")).is_ok());
        for (id, url) in [
            ("../../x", "https://h/ok.gguf"),
            ("a/b", "https://h/ok.gguf"),
            ("a\\\\b", "https://h/ok.gguf"),
            ("custom", "https://h/ok.gguf"),
            (".hidden", "https://h/ok.gguf"),
            ("ok", "https://h/x/.."),
            ("ok", "https://h/x\\\\..\\\\y"),
        ] {
            assert!(
                matches!(
                    Manifest::parse(&entry(id, url)),
                    Err(ModelError::Manifest(_))
                ),
                "{id} {url}"
            );
        }
    }

    #[test]
    fn file_name_comes_from_url() {
        let m = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(m.get("a").unwrap().file_name(), "ggml-a.bin");
    }
}
