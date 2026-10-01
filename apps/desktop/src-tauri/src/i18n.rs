//! Strings shown by the Rust side (tray menu, widget messages). The
//! frontend has its own JSON tables; the test below keeps the two message
//! sets aligned by name.

use crate::settings::UiLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    ZhTw,
    En,
}

impl Language {
    pub fn from_setting(setting: UiLanguage, system_preferred: Option<&str>) -> Language {
        match setting {
            UiLanguage::ZhTw => Language::ZhTw,
            UiLanguage::En => Language::En,
            UiLanguage::System => match system_preferred {
                Some(tag)
                    if tag.starts_with("zh-Hant") || tag == "zh-TW" || tag.starts_with("zh-TW") =>
                {
                    Language::ZhTw
                }
                _ => Language::En,
            },
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Language::ZhTw => "zh-TW",
            Language::En => "en",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Msg {
    TrayStatusIdle,
    TrayStatusRecording,
    TrayStatusProcessing,
    TrayStatusNoAccessibility,
    TrayStatusEngineError,
    TraySettings,
    TrayHistory,
    TrayModels,
    TrayLaunchAtLogin,
    TrayQuit,
    WidgetListening,
    WidgetLocked,
    WidgetProcessing,
    WidgetInserting,
    WidgetNoSpeech,
    WidgetCancelled,
    WidgetSecureInput,
    WidgetAppChanged,
    WidgetEmptyResult,
    WidgetAccessibilityRequired,
    WidgetMicrophoneUnavailable,
    WidgetEngineUnavailable,
    WidgetAsrFailed,
    WidgetModelMissing,
    WidgetModelCorrupt,
    WidgetNativeUnavailable,
    WidgetDegraded,
    WidgetCopy,
    WidgetDismiss,
}

pub const ALL_MSGS: &[Msg] = &[
    Msg::TrayStatusIdle,
    Msg::TrayStatusRecording,
    Msg::TrayStatusProcessing,
    Msg::TrayStatusNoAccessibility,
    Msg::TrayStatusEngineError,
    Msg::TraySettings,
    Msg::TrayHistory,
    Msg::TrayModels,
    Msg::TrayLaunchAtLogin,
    Msg::TrayQuit,
    Msg::WidgetListening,
    Msg::WidgetLocked,
    Msg::WidgetProcessing,
    Msg::WidgetInserting,
    Msg::WidgetNoSpeech,
    Msg::WidgetCancelled,
    Msg::WidgetSecureInput,
    Msg::WidgetAppChanged,
    Msg::WidgetEmptyResult,
    Msg::WidgetAccessibilityRequired,
    Msg::WidgetMicrophoneUnavailable,
    Msg::WidgetEngineUnavailable,
    Msg::WidgetAsrFailed,
    Msg::WidgetModelMissing,
    Msg::WidgetModelCorrupt,
    Msg::WidgetNativeUnavailable,
    Msg::WidgetDegraded,
    Msg::WidgetCopy,
    Msg::WidgetDismiss,
];

impl Msg {
    /// Key used by the frontend JSON tables for the same message.
    pub fn key(self) -> &'static str {
        match self {
            Msg::TrayStatusIdle => "tray.status.idle",
            Msg::TrayStatusRecording => "tray.status.recording",
            Msg::TrayStatusProcessing => "tray.status.processing",
            Msg::TrayStatusNoAccessibility => "tray.status.no_accessibility",
            Msg::TrayStatusEngineError => "tray.status.engine_error",
            Msg::TraySettings => "tray.settings",
            Msg::TrayHistory => "tray.history",
            Msg::TrayModels => "tray.models",
            Msg::TrayLaunchAtLogin => "tray.launch_at_login",
            Msg::TrayQuit => "tray.quit",
            Msg::WidgetListening => "widget.listening",
            Msg::WidgetLocked => "widget.locked",
            Msg::WidgetProcessing => "widget.processing",
            Msg::WidgetInserting => "widget.inserting",
            Msg::WidgetNoSpeech => "widget.no_speech",
            Msg::WidgetCancelled => "widget.cancelled",
            Msg::WidgetSecureInput => "widget.secure_input",
            Msg::WidgetAppChanged => "widget.app_changed",
            Msg::WidgetEmptyResult => "widget.empty_result",
            Msg::WidgetAccessibilityRequired => "widget.accessibility_required",
            Msg::WidgetMicrophoneUnavailable => "widget.microphone_unavailable",
            Msg::WidgetEngineUnavailable => "widget.engine_unavailable",
            Msg::WidgetAsrFailed => "widget.asr_failed",
            Msg::WidgetModelMissing => "widget.model_missing",
            Msg::WidgetModelCorrupt => "widget.model_corrupt",
            Msg::WidgetNativeUnavailable => "widget.native_unavailable",
            Msg::WidgetDegraded => "widget.degraded",
            Msg::WidgetCopy => "widget.copy",
            Msg::WidgetDismiss => "widget.dismiss",
        }
    }
}

pub fn text(lang: Language, msg: Msg) -> &'static str {
    match (lang, msg) {
        (Language::ZhTw, Msg::TrayStatusIdle) => "待命中",
        (Language::En, Msg::TrayStatusIdle) => "Idle",
        (Language::ZhTw, Msg::TrayStatusRecording) => "錄音中",
        (Language::En, Msg::TrayStatusRecording) => "Recording",
        (Language::ZhTw, Msg::TrayStatusProcessing) => "處理中",
        (Language::En, Msg::TrayStatusProcessing) => "Processing",
        (Language::ZhTw, Msg::TrayStatusNoAccessibility) => "需要輔助使用權限",
        (Language::En, Msg::TrayStatusNoAccessibility) => "Accessibility permission required",
        (Language::ZhTw, Msg::TrayStatusEngineError) => "引擎無法啟動",
        (Language::En, Msg::TrayStatusEngineError) => "Engine failed to start",
        (Language::ZhTw, Msg::TraySettings) => "設定...",
        (Language::En, Msg::TraySettings) => "Settings...",
        (Language::ZhTw, Msg::TrayHistory) => "歷史紀錄...",
        (Language::En, Msg::TrayHistory) => "History...",
        (Language::ZhTw, Msg::TrayModels) => "引擎與模型...",
        (Language::En, Msg::TrayModels) => "Engines and models...",
        (Language::ZhTw, Msg::TrayLaunchAtLogin) => "登入時啟動",
        (Language::En, Msg::TrayLaunchAtLogin) => "Launch at Login",
        (Language::ZhTw, Msg::TrayQuit) => "結束 Kongwe",
        (Language::En, Msg::TrayQuit) => "Quit Kongwe",
        (Language::ZhTw, Msg::WidgetListening) => "聆聽中",
        (Language::En, Msg::WidgetListening) => "Listening",
        (Language::ZhTw, Msg::WidgetLocked) => "連續聽寫中，再按 fn 結束",
        (Language::En, Msg::WidgetLocked) => "Hands-free, press fn to finish",
        (Language::ZhTw, Msg::WidgetProcessing) => "處理中",
        (Language::En, Msg::WidgetProcessing) => "Processing",
        (Language::ZhTw, Msg::WidgetInserting) => "輸入中",
        (Language::En, Msg::WidgetInserting) => "Inserting",
        (Language::ZhTw, Msg::WidgetNoSpeech) => "沒有偵測到語音",
        (Language::En, Msg::WidgetNoSpeech) => "No speech detected",
        (Language::ZhTw, Msg::WidgetCancelled) => "已取消",
        (Language::En, Msg::WidgetCancelled) => "Cancelled",
        (Language::ZhTw, Msg::WidgetSecureInput) => "目前欄位為安全輸入，無法自動填入",
        (Language::En, Msg::WidgetSecureInput) => "Secure input is active; text cannot be inserted",
        (Language::ZhTw, Msg::WidgetAppChanged) => "前景應用程式已切換，未自動填入",
        (Language::En, Msg::WidgetAppChanged) => "The frontmost app changed; text was not inserted",
        (Language::ZhTw, Msg::WidgetEmptyResult) => "沒有產生文字",
        (Language::En, Msg::WidgetEmptyResult) => "No text was produced",
        (Language::ZhTw, Msg::WidgetAccessibilityRequired) => {
            "請在系統設定允許 Kongwe 的輔助使用權限"
        }
        (Language::En, Msg::WidgetAccessibilityRequired) => {
            "Allow Kongwe under Accessibility in System Settings"
        }
        (Language::ZhTw, Msg::WidgetMicrophoneUnavailable) => "無法使用麥克風",
        (Language::En, Msg::WidgetMicrophoneUnavailable) => "Microphone unavailable",
        (Language::ZhTw, Msg::WidgetEngineUnavailable) => "引擎尚未就緒，請開啟設定",
        (Language::En, Msg::WidgetEngineUnavailable) => "Engine not ready; open Settings",
        (Language::ZhTw, Msg::WidgetAsrFailed) => "語音辨識失敗，未插入文字",
        (Language::En, Msg::WidgetAsrFailed) => "Speech recognition failed; nothing was inserted",
        (Language::ZhTw, Msg::WidgetModelMissing) => "找不到本機模型，請到設定 > 引擎與模型下載",
        (Language::En, Msg::WidgetModelMissing) => {
            "Local model not found; download it in Settings > Engines and models"
        }
        (Language::ZhTw, Msg::WidgetModelCorrupt) => "模型檔案損毀，請重新下載",
        (Language::En, Msg::WidgetModelCorrupt) => "Model file is damaged; download it again",
        (Language::ZhTw, Msg::WidgetNativeUnavailable) => "Apple 的服務目前無法使用",
        (Language::En, Msg::WidgetNativeUnavailable) => "The Apple service is not available",
        (Language::ZhTw, Msg::WidgetDegraded) => "改寫失敗，已插入原始辨識結果",
        (Language::En, Msg::WidgetDegraded) => "Rewrite failed; inserted the raw transcript",
        (Language::ZhTw, Msg::WidgetCopy) => "複製",
        (Language::En, Msg::WidgetCopy) => "Copy",
        (Language::ZhTw, Msg::WidgetDismiss) => "關閉",
        (Language::En, Msg::WidgetDismiss) => "Dismiss",
    }
}

#[cfg(target_os = "macos")]
pub fn system_preferred_language() -> Option<String> {
    let langs = objc2_foundation::NSLocale::preferredLanguages();
    if langs.count() == 0 {
        return None;
    }
    Some(langs.objectAtIndex(0).to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn system_preferred_language() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_setting_follows_the_preferred_language() {
        assert_eq!(
            Language::from_setting(UiLanguage::System, Some("zh-Hant-TW")),
            Language::ZhTw
        );
        assert_eq!(
            Language::from_setting(UiLanguage::System, Some("zh-TW")),
            Language::ZhTw
        );
        assert_eq!(
            Language::from_setting(UiLanguage::System, Some("en-US")),
            Language::En
        );
        assert_eq!(
            Language::from_setting(UiLanguage::System, Some("ja-JP")),
            Language::En
        );
        assert_eq!(
            Language::from_setting(UiLanguage::System, None),
            Language::En
        );
        assert_eq!(
            Language::from_setting(UiLanguage::ZhTw, Some("en")),
            Language::ZhTw
        );
        assert_eq!(
            Language::from_setting(UiLanguage::En, Some("zh-Hant")),
            Language::En
        );
    }

    #[test]
    fn every_message_has_both_languages_and_a_unique_key() {
        let mut keys = std::collections::HashSet::new();
        for m in ALL_MSGS {
            assert!(!text(Language::ZhTw, *m).is_empty());
            assert!(!text(Language::En, *m).is_empty());
            assert!(keys.insert(m.key()), "duplicate key {}", m.key());
        }
    }

    #[test]
    fn rust_message_keys_exist_in_both_frontend_tables() {
        let zh: serde_json::Value =
            serde_json::from_str(include_str!("../../src/lib/i18n/zh-TW.json")).unwrap();
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../src/lib/i18n/en.json")).unwrap();
        for m in ALL_MSGS {
            assert!(zh.get(m.key()).is_some(), "zh-TW.json lacks {}", m.key());
            assert!(en.get(m.key()).is_some(), "en.json lacks {}", m.key());
            assert_eq!(
                zh[m.key()],
                text(Language::ZhTw, *m),
                "zh text differs for {}",
                m.key()
            );
            assert_eq!(
                en[m.key()],
                text(Language::En, *m),
                "en text differs for {}",
                m.key()
            );
        }
    }
}
