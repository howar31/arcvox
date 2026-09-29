use super::ModelError;
use crate::config::Runtime;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
}

impl ModelEntry {
    /// Last path segment of the download URL; `<id>.bin` when there is none.
    pub fn file_name(&self) -> String {
        let last = self
            .url
            .split(['?', '#'])
            .next()
            .and_then(|u| u.rsplit('/').next())
            .unwrap_or("");
        if last.is_empty() {
            format!("{}.bin", self.id)
        } else {
            last.to_string()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub models: Vec<ModelEntry>,
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Manifest, ModelError> {
        let m: Manifest =
            serde_json::from_str(json).map_err(|e| ModelError::Manifest(e.to_string()))?;
        if m.schema_version > MANIFEST_SCHEMA_VERSION {
            return Err(ModelError::Manifest(format!(
                "schema version {} is newer than supported version {}",
                m.schema_version, MANIFEST_SCHEMA_VERSION
            )));
        }
        for e in &m.models {
            if !safe_segment(&e.id) || e.id == "custom" || !safe_segment(&e.file_name()) {
                return Err(ModelError::Manifest(format!(
                    "model `{}` has an unsafe id or file name",
                    e.id
                )));
            }
        }
        Ok(m)
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn recommended(&self, role: ModelRole) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.recommended && m.role == role)
    }
}

/// Where the manifest in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
