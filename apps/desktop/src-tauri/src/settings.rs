//! Client-only settings in `client.json` next to core's `config.json`.
//! Missing file = defaults; unparsable file is moved aside and defaults are used.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::shortcut::Shortcut;

pub const CLIENT_SCHEMA_VERSION: u32 = 1;
pub const FILE_NAME: &str = "client.json";

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientSettings {
    pub schema_version: u32,
    pub hotkey: HotkeySettings,
    /// `"builtin"`, `"default"`, or a device name.
    pub microphone: String,
    pub context: ContextOptIns,
    pub insertion: InsertionSettings,
    pub max_utterance_secs: u32,
    pub auto_stop: AutoStop,
    /// Shows the recognised text next to the widget while it is rewritten.
    pub show_transcript: bool,
    /// How large the floating widget and its panel are.
    pub widget_size: WidgetSize,
    pub ui_language: UiLanguage,
    /// Overrides the model catalogue location; `None` uses the built-in one.
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// Where each card of the statistics page sits and how large it is;
    /// `None` is the built-in arrangement. The page validates it.
    #[serde(default)]
    pub stats_layout: Option<Vec<StatsCard>>,
}

/// One card of the statistics page on its grid of three columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsCard {
    pub id: String,
    pub row: u32,
    pub col: u32,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HotkeySettings {
    pub hold_threshold_ms: u64,
    pub double_tap_window_ms: u64,
    /// The key combinations and mouse buttons that start a recording.
    #[serde(deserialize_with = "crate::shortcut::lenient")]
    pub shortcuts: Vec<Shortcut>,
}

/// Ends a hands-free dictation by itself after a pause in the speech.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutoStop {
    pub enabled: bool,
    pub silence_secs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl WidgetSize {
    /// The factor on the small size's measures.
    pub fn scale(self) -> f64 {
        match self {
            WidgetSize::Small => 1.0,
            WidgetSize::Medium => 7.0 / 6.0,
            WidgetSize::Large => 4.0 / 3.0,
        }
    }
}

impl Default for AutoStop {
    fn default() -> Self {
        Self {
            enabled: false,
            silence_secs: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextOptIns {
    pub app_name: bool,
    pub window_title: bool,
    pub url: bool,
    pub selected_text: bool,
    pub clipboard: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InsertionSettings {
    /// Bundle id -> method. Apps not listed use `Paste`.
    pub per_app: BTreeMap<String, InsertMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertMethod {
    Paste,
    Type,
    Ax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiLanguage {
    System,
    ZhTw,
    En,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicrophoneChoice {
    BuiltIn,
    SystemDefault,
    Named(String),
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            schema_version: CLIENT_SCHEMA_VERSION,
            hotkey: HotkeySettings::default(),
            microphone: "builtin".into(),
            context: ContextOptIns::default(),
            insertion: InsertionSettings::default(),
            max_utterance_secs: 300,
            auto_stop: AutoStop::default(),
            show_transcript: true,
            widget_size: WidgetSize::default(),
            ui_language: UiLanguage::System,
            manifest_url: None,
            stats_layout: None,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            hold_threshold_ms: 250,
            double_tap_window_ms: 350,
            shortcuts: vec![Shortcut::fn_key()],
        }
    }
}

impl Default for ContextOptIns {
    fn default() -> Self {
        Self {
            app_name: true,
            window_title: true,
            url: true,
            selected_text: false,
            clipboard: false,
        }
    }
}

impl ClientSettings {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join(FILE_NAME)
    }

    /// Never fails: a missing file yields defaults; a broken file is renamed
    /// to `client.json.broken-<unix ts>` and defaults are returned.
    pub fn load(config_dir: &Path) -> ClientSettings {
        let path = Self::path(config_dir);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str::<ClientSettings>(&text) {
            Ok(mut s) => {
                s.schema_version = CLIENT_SCHEMA_VERSION;
                s
            }
            Err(e) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let backup = config_dir.join(format!("{FILE_NAME}.broken-{ts}"));
                let _ = std::fs::rename(&path, &backup);
                tracing::warn!(?backup, error = %e, "client settings unparsable, moved aside");
                Self::default()
            }
        }
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save(&self, config_dir: &Path) -> Result<(), SettingsError> {
        let path = Self::path(config_dir);
        let io = |source| SettingsError::Io {
            path: path.clone(),
            source,
        };
        std::fs::create_dir_all(config_dir).map_err(io)?;
        let tmp = config_dir.join(format!("{FILE_NAME}.tmp"));
        let bytes = serde_json::to_vec_pretty(self).expect("settings serialize");
        std::fs::write(&tmp, bytes).map_err(io)?;
        std::fs::rename(&tmp, &path).map_err(io)?;
        Ok(())
    }

    pub fn microphone_choice(&self) -> MicrophoneChoice {
        match self.microphone.as_str() {
            "builtin" => MicrophoneChoice::BuiltIn,
            "default" => MicrophoneChoice::SystemDefault,
            name => MicrophoneChoice::Named(name.to_string()),
        }
    }

    pub fn method_for(&self, bundle_id: Option<&str>) -> InsertMethod {
        bundle_id
            .and_then(|id| self.insertion.per_app.get(id).copied())
            .unwrap_or(InsertMethod::Paste)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_design() {
        let s = ClientSettings::default();
        assert_eq!(s.hotkey.hold_threshold_ms, 250);
        assert_eq!(s.hotkey.double_tap_window_ms, 350);
        assert_eq!(s.max_utterance_secs, 300);
        assert_eq!(s.microphone_choice(), MicrophoneChoice::BuiltIn);
        assert!(s.context.app_name && s.context.window_title && s.context.url);
        assert!(!s.context.selected_text && !s.context.clipboard);
        assert_eq!(
            s.method_for(Some("com.apple.Terminal")),
            InsertMethod::Paste
        );
        assert_eq!(s.ui_language, UiLanguage::System);
    }

    #[test]
    fn manifest_url_defaults_to_none_and_round_trips() {
        let s: ClientSettings =
            serde_json::from_str(&serde_json::to_string(&ClientSettings::default()).unwrap())
                .unwrap();
        assert!(s.manifest_url.is_none());
    }

    #[test]
    fn missing_file_yields_defaults_and_save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = ClientSettings::load(dir.path());
        assert_eq!(loaded, ClientSettings::default());
        let mut s = loaded;
        s.microphone = "External USB".into();
        s.insertion
            .per_app
            .insert("com.apple.Terminal".into(), InsertMethod::Type);
        s.save(dir.path()).unwrap();
        let again = ClientSettings::load(dir.path());
        assert_eq!(again, s);
        assert_eq!(
            again.microphone_choice(),
            MicrophoneChoice::Named("External USB".into())
        );
        assert_eq!(
            again.method_for(Some("com.apple.Terminal")),
            InsertMethod::Type
        );
        assert!(!dir.path().join("client.json.tmp").exists());
    }

    #[test]
    fn broken_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(ClientSettings::path(dir.path()), b"{ not json").unwrap();
        let s = ClientSettings::load(dir.path());
        assert_eq!(s, ClientSettings::default());
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("client.json.broken-")),
            "{names:?}"
        );
        assert!(!names.iter().any(|n| n == "client.json"));
    }

    #[test]
    fn json_shape_is_stable() {
        let json = serde_json::to_value(ClientSettings::default()).unwrap();
        assert_eq!(json["ui_language"], "system");
        assert_eq!(json["hotkey"]["hold_threshold_ms"], 250);
        let with_method: ClientSettings = serde_json::from_value(serde_json::json!({
            "insertion": {"per_app": {"com.example": "ax"}}
        }))
        .unwrap();
        assert_eq!(
            with_method.method_for(Some("com.example")),
            InsertMethod::Ax
        );
    }
}
