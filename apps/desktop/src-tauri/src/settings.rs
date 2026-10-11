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
    /// What the floating widget shows while no dictation goes on.
    pub widget_idle: WidgetIdle,
    /// Whether and how the widget turns faint after resting a while.
    pub widget_fade: WidgetFade,
    /// The display the floating widget stays on, by the name its position
    /// is saved under; `None` shows it on the display under the pointer.
    pub widget_display: Option<String>,
    pub ui_language: UiLanguage,
    /// Overrides the model catalogue location; `None` uses the built-in one.
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// Where each card of the statistics page sits and how large it is;
    /// `None` is the built-in arrangement. The page validates it.
    #[serde(default)]
    pub stats_layout: Option<Vec<StatsCard>>,
    /// The backup folder and what is kept up to date in it.
    pub backup: BackupSettings,
}

/// The folder the app writes this device's history, statistics and
/// recordings to, for the user's own backup or sync.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupSettings {
    pub folder: Option<PathBuf>,
    pub categories: BackupCategories,
    /// Writes to the folder by itself a few minutes after a change.
    pub auto: bool,
    /// Makes the folder again when a write finds it gone.
    pub recreate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupCategories {
    pub history: bool,
    pub stats: bool,
    pub recordings: bool,
    /// A copy of this device's settings, as a settings file.
    pub settings: bool,
    /// This device's content (profiles, engines, dictionary, base prompt)
    /// in that same file.
    pub content: bool,
}

impl Default for BackupCategories {
    fn default() -> Self {
        Self {
            history: true,
            stats: true,
            recordings: false,
            settings: true,
            content: true,
        }
    }
}

impl From<BackupCategories> for arcvox_core::backup::Categories {
    fn from(c: BackupCategories) -> Self {
        Self {
            history: c.history,
            stats: c.stats,
            recordings: c.recordings,
            // One file holds the settings and the content.
            settings: c.settings || c.content,
        }
    }
}

/// The sections of a settings file this client fills and applies. `general`
/// is shared with the core: this client's part is its `client` member.
pub const CLIENT_SECTIONS: [&str; 4] = ["recording", "insertion", "general", "appearance"];

#[derive(Serialize, Deserialize)]
struct RecordingSection {
    hotkey: HotkeySettings,
    max_utterance_secs: u32,
    auto_stop: AutoStop,
}

#[derive(Serialize, Deserialize)]
struct GeneralSection {
    context: ContextOptIns,
    ui_language: UiLanguage,
    #[serde(default)]
    manifest_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct AppearanceSection {
    widget_size: WidgetSize,
    widget_idle: WidgetIdle,
    #[serde(default)]
    widget_fade: WidgetFade,
    show_transcript: bool,
    #[serde(default)]
    stats_layout: Option<Vec<StatsCard>>,
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
    /// Escape also cancels a dictation that is being processed, not only
    /// one being recorded.
    pub escape_cancels_processing: bool,
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
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
    XLarge,
}

/// What the floating widget shows while no dictation goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetIdle {
    /// Nothing: the widget leaves the screen.
    #[default]
    Hidden,
    /// The ring alone, in the round end of the widget.
    Ring,
    /// The whole widget, with a label.
    Label,
}

/// A widget that rests on the screen turns faint after a while, and comes
/// back when a dictation begins or the pointer is over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WidgetFade {
    pub enabled: bool,
    /// How long the widget rests before it turns faint.
    pub after_secs: u32,
    /// How transparent the faint widget is, in percent.
    pub transparency: u32,
}

impl Default for WidgetFade {
    fn default() -> Self {
        Self {
            enabled: true,
            after_secs: 2,
            transparency: 70,
        }
    }
}

impl WidgetFade {
    /// The values kept inside what the settings page offers.
    pub fn bounded(self) -> Self {
        Self {
            enabled: self.enabled,
            after_secs: self.after_secs.clamp(1, 60),
            transparency: self.transparency.clamp(10, 90),
        }
    }
}

impl WidgetIdle {
    pub const ALL: [WidgetIdle; 3] = [WidgetIdle::Hidden, WidgetIdle::Ring, WidgetIdle::Label];

    pub fn name(self) -> &'static str {
        match self {
            WidgetIdle::Hidden => "hidden",
            WidgetIdle::Ring => "ring",
            WidgetIdle::Label => "label",
        }
    }
}

impl WidgetSize {
    /// The factor on the widget's base measures, a short side of 48.
    pub fn scale(self) -> f64 {
        match self {
            WidgetSize::XSmall => 7.0 / 12.0,
            WidgetSize::Small => 0.75,
            WidgetSize::Medium => 11.0 / 12.0,
            WidgetSize::Large => 7.0 / 6.0,
            WidgetSize::XLarge => 1.5,
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
    /// The method of every app that is not in `per_app`.
    pub default: InsertMethod,
    /// The final text stays on the clipboard, whatever the method.
    pub keep_on_clipboard: bool,
    /// Bundle id -> method.
    pub per_app: BTreeMap<String, InsertMethod>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertMethod {
    #[default]
    Paste,
    Type,
    Ax,
    /// Nothing is put into the application; the text goes to the clipboard.
    Copy,
}

impl InsertMethod {
    /// The method as the settings file and the history name it.
    pub fn name(self) -> &'static str {
        match self {
            InsertMethod::Paste => "paste",
            InsertMethod::Type => "type",
            InsertMethod::Ax => "ax",
            InsertMethod::Copy => "copy",
        }
    }
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
            widget_idle: WidgetIdle::default(),
            widget_fade: WidgetFade::default(),
            widget_display: None,
            ui_language: UiLanguage::System,
            manifest_url: None,
            stats_layout: None,
            backup: BackupSettings::default(),
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            hold_threshold_ms: 250,
            double_tap_window_ms: 350,
            escape_cancels_processing: true,
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

    /// This client's part of the named sections, for a settings file. The
    /// microphone, the widget's display and the backup folder name things
    /// of this machine and are never part of it.
    pub fn export_sections(&self, names: &[String]) -> BTreeMap<String, serde_json::Value> {
        let has = |name: &str| names.iter().any(|n| n == name);
        let value = |v: serde_json::Result<serde_json::Value>| v.expect("settings serialize");
        let mut out = BTreeMap::new();
        if has("recording") {
            let section = RecordingSection {
                hotkey: self.hotkey.clone(),
                max_utterance_secs: self.max_utterance_secs,
                auto_stop: self.auto_stop.clone(),
            };
            out.insert(
                "recording".to_string(),
                value(serde_json::to_value(section)),
            );
        }
        if has("insertion") {
            out.insert(
                "insertion".to_string(),
                value(serde_json::to_value(&self.insertion)),
            );
        }
        if has("general") {
            let section = GeneralSection {
                context: self.context.clone(),
                ui_language: self.ui_language,
                manifest_url: self.manifest_url.clone(),
            };
            out.insert(
                "general".to_string(),
                serde_json::json!({ "client": value(serde_json::to_value(section)) }),
            );
        }
        if has("appearance") {
            let section = AppearanceSection {
                widget_size: self.widget_size,
                widget_idle: self.widget_idle,
                widget_fade: self.widget_fade,
                show_transcript: self.show_transcript,
                stats_layout: self.stats_layout.clone(),
            };
            out.insert(
                "appearance".to_string(),
                value(serde_json::to_value(section)),
            );
        }
        out
    }

    /// This client's part of a section of `file`, when the section is named
    /// and the file has it.
    fn client_part<'a>(
        file: &'a arcvox_core::settings_file::SettingsFile,
        names: &[String],
        section: &str,
    ) -> Option<&'a serde_json::Value> {
        let value = names
            .iter()
            .any(|n| n == section)
            .then(|| file.sections.get(section))
            .flatten()?;
        if section == "general" {
            value.get("client")
        } else {
            Some(value)
        }
    }

    /// The sections of `names` that `with_sections` takes from `file`.
    pub fn applied_sections(
        file: &arcvox_core::settings_file::SettingsFile,
        names: &[String],
    ) -> Vec<String> {
        CLIENT_SECTIONS
            .iter()
            .filter(|s| Self::client_part(file, names, s).is_some())
            .map(|s| s.to_string())
            .collect()
    }

    /// A copy with the named sections taken from `file`. A section the file
    /// lacks is left as it is; one that cannot be read is an error and
    /// nothing is taken.
    pub fn with_sections(
        &self,
        file: &arcvox_core::settings_file::SettingsFile,
        names: &[String],
    ) -> Result<ClientSettings, String> {
        fn read<T: serde::de::DeserializeOwned>(
            section: &str,
            value: &serde_json::Value,
        ) -> Result<T, String> {
            serde_json::from_value(value.clone())
                .map_err(|e| format!("the section `{section}` cannot be read: {e}"))
        }
        let mut next = self.clone();
        if let Some(v) = Self::client_part(file, names, "recording") {
            let section: RecordingSection = read("recording", v)?;
            next.hotkey = section.hotkey;
            next.max_utterance_secs = section.max_utterance_secs;
            next.auto_stop = section.auto_stop;
        }
        if let Some(v) = Self::client_part(file, names, "insertion") {
            next.insertion = read("insertion", v)?;
        }
        if let Some(v) = Self::client_part(file, names, "general") {
            let section: GeneralSection = read("general", v)?;
            next.context = section.context;
            next.ui_language = section.ui_language;
            next.manifest_url = section.manifest_url;
        }
        if let Some(v) = Self::client_part(file, names, "appearance") {
            let section: AppearanceSection = read("appearance", v)?;
            next.widget_size = section.widget_size;
            next.widget_idle = section.widget_idle;
            next.widget_fade = section.widget_fade;
            next.show_transcript = section.show_transcript;
            next.stats_layout = section.stats_layout;
        }
        Ok(next)
    }

    /// Copies the settings file aside as `client.json.bak-<seconds>` and
    /// returns the copy; `None` when there is no file yet.
    pub fn copy_aside(config_dir: &Path) -> Result<Option<PathBuf>, SettingsError> {
        let path = Self::path(config_dir);
        if !path.exists() {
            return Ok(None);
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut copy = config_dir.join(format!("{FILE_NAME}.bak-{ts}"));
        let mut n = 1;
        while copy.exists() {
            n += 1;
            copy = config_dir.join(format!("{FILE_NAME}.bak-{ts}-{n}"));
        }
        std::fs::copy(&path, &copy).map_err(|source| SettingsError::Io {
            path: copy.clone(),
            source,
        })?;
        Ok(Some(copy))
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
            .unwrap_or(self.insertion.default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_the_fade_gets_the_default_and_values_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("client.json"), br#"{"schema_version":1}"#).unwrap();
        let got = ClientSettings::load(dir.path());
        assert_eq!(
            got.widget_fade,
            WidgetFade {
                enabled: true,
                after_secs: 2,
                transparency: 70
            }
        );
        let wild = WidgetFade {
            enabled: true,
            after_secs: 0,
            transparency: 100,
        };
        assert_eq!(
            (wild.bounded().after_secs, wild.bounded().transparency),
            (1, 90)
        );
    }

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
        assert_eq!(json["insertion"]["default"], "paste");
        assert_eq!(json["insertion"]["keep_on_clipboard"], false);
    }

    #[test]
    fn apps_not_listed_use_the_default_method() {
        let s: ClientSettings = serde_json::from_value(serde_json::json!({
            "insertion": {"default": "copy", "per_app": {"com.example": "type"}}
        }))
        .unwrap();
        assert_eq!(s.method_for(Some("com.example")), InsertMethod::Type);
        assert_eq!(s.method_for(Some("com.other")), InsertMethod::Copy);
        assert_eq!(s.method_for(None), InsertMethod::Copy);
    }

    use arcvox_core::settings_file::SettingsFile;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn changed() -> ClientSettings {
        let mut s = ClientSettings::default();
        s.hotkey.hold_threshold_ms = 400;
        s.max_utterance_secs = 120;
        s.auto_stop.enabled = true;
        s.insertion.default = InsertMethod::Type;
        s.insertion.keep_on_clipboard = true;
        s.insertion
            .per_app
            .insert("com.apple.Terminal".into(), InsertMethod::Ax);
        s.context.clipboard = true;
        s.ui_language = UiLanguage::En;
        s.widget_size = WidgetSize::Large;
        s.widget_idle = WidgetIdle::Ring;
        s.show_transcript = false;
        s.microphone = "Studio Mic 9".into();
        s.widget_display = Some("Monitor Nine".into());
        s.backup.folder = Some("/Volumes/Nine/backup".into());
        s.backup.auto = true;
        s
    }

    fn file_with(s: &ClientSettings, sections: &[&str]) -> SettingsFile {
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        file.sections = s.export_sections(&names(sections));
        file
    }

    #[test]
    fn the_file_is_copied_aside_before_an_import() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ClientSettings::copy_aside(dir.path()).unwrap(), None);
        let saved = changed();
        saved.save(dir.path()).unwrap();
        let copy = ClientSettings::copy_aside(dir.path()).unwrap().unwrap();
        assert!(copy
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("client.json.bak-"));
        ClientSettings::default().save(dir.path()).unwrap();
        let kept: ClientSettings = serde_json::from_slice(&std::fs::read(&copy).unwrap()).unwrap();
        assert_eq!(kept, saved);
        // A second copy in the same second does not replace the first.
        let second = ClientSettings::copy_aside(dir.path()).unwrap().unwrap();
        assert_ne!(second, copy);
    }

    #[test]
    fn backup_defaults() {
        let b = ClientSettings::default().backup;
        assert_eq!(b.folder, None);
        assert!(!b.auto);
        assert!(!b.recreate);
        assert!(b.categories.history && b.categories.stats);
        assert!(b.categories.settings && b.categories.content);
        assert!(!b.categories.recordings);
        let core: arcvox_core::backup::Categories = b.categories.into();
        assert!(core.settings);
        let records_only = BackupCategories {
            settings: false,
            content: false,
            ..b.categories
        };
        assert!(!arcvox_core::backup::Categories::from(records_only).settings);
        let content_only = BackupCategories {
            settings: false,
            ..b.categories
        };
        assert!(arcvox_core::backup::Categories::from(content_only).settings);
    }

    #[test]
    fn an_older_file_without_backup_loads() {
        let mut v = serde_json::to_value(ClientSettings::default()).unwrap();
        v.as_object_mut().unwrap().remove("backup");
        let s: ClientSettings = serde_json::from_value(v).unwrap();
        assert_eq!(s.backup, BackupSettings::default());
    }

    #[test]
    fn client_sections_round_trip() {
        let source = changed();
        let file = file_with(&source, &CLIENT_SECTIONS);
        let got = ClientSettings::default()
            .with_sections(&file, &names(&CLIENT_SECTIONS))
            .unwrap();
        assert_eq!(got.hotkey, source.hotkey);
        assert_eq!(got.max_utterance_secs, 120);
        assert_eq!(got.auto_stop, source.auto_stop);
        assert_eq!(got.insertion, source.insertion);
        assert_eq!(got.context, source.context);
        assert_eq!(got.ui_language, UiLanguage::En);
        assert_eq!(
            (got.widget_size, got.widget_idle, got.show_transcript),
            (WidgetSize::Large, WidgetIdle::Ring, false)
        );
    }

    #[test]
    fn the_microphone_the_display_and_the_backup_folder_never_travel() {
        let file = file_with(&changed(), &CLIENT_SECTIONS);
        let text = String::from_utf8(file.to_bytes()).unwrap();
        for private in ["Studio Mic 9", "Monitor Nine", "/Volumes/Nine"] {
            assert!(!text.contains(private), "{private}");
        }
        let mut here = ClientSettings {
            microphone: "Desk Mic".into(),
            widget_display: Some("Left".into()),
            ..ClientSettings::default()
        };
        here.backup.folder = Some("/here".into());
        let got = here.with_sections(&file, &names(&CLIENT_SECTIONS)).unwrap();
        assert_eq!(got.microphone, "Desk Mic");
        assert_eq!(got.widget_display.as_deref(), Some("Left"));
        assert_eq!(got.backup, here.backup);
    }

    #[test]
    fn only_the_sections_named_and_present_are_taken() {
        let source = changed();
        let file = file_with(&source, &["insertion", "appearance"]);
        let got = ClientSettings::default()
            .with_sections(&file, &names(&["insertion", "recording"]))
            .unwrap();
        assert_eq!(got.insertion, source.insertion);
        assert_eq!(got.widget_size, WidgetSize::default());
        assert_eq!(got.hotkey, HotkeySettings::default());
        assert_eq!(
            ClientSettings::applied_sections(&file, &names(&["insertion", "recording"])),
            names(&["insertion"])
        );
    }

    #[test]
    fn the_general_section_keeps_the_part_of_the_core() {
        let file = file_with(&changed(), &["general"]);
        assert!(file.sections["general"].get("client").is_some());
        let mut core_only = SettingsFile::new("0.1.0", "macos", "Anna");
        core_only
            .sections
            .insert("general".into(), serde_json::json!({ "core": {} }));
        let got = ClientSettings::default()
            .with_sections(&core_only, &names(&["general"]))
            .unwrap();
        assert_eq!(got, ClientSettings::default());
        assert!(ClientSettings::applied_sections(&core_only, &names(&["general"])).is_empty());
    }

    #[test]
    fn a_section_of_the_wrong_shape_is_an_error() {
        let mut file = SettingsFile::new("0.1.0", "macos", "Anna");
        file.sections
            .insert("recording".into(), serde_json::json!(5));
        let got = ClientSettings::default().with_sections(&file, &names(&["recording"]));
        assert!(got.unwrap_err().contains("recording"));
    }
}
