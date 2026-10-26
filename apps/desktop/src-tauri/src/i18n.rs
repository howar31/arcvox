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
                Some(tag) if tag.starts_with("zh-Hant") || tag.starts_with("zh-TW") => {
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
    TrayStatusPaused,
    TrayPause,
    TraySettings,
    TrayAbout,
    TrayUpdateCheck,
    TrayUpdateFound,
    TrayCopyLast,
    TrayHistory,
    TrayStatistics,
    TrayProfiles,
    TrayProfilesManage,
    TrayWidget,
    TrayWidgetPlace,
    TrayWidgetIdle,
    TrayWidgetIdleHidden,
    TrayWidgetIdleRing,
    TrayWidgetIdleLabel,
    TrayFixPermissions,
    TrayMicrophoneDenied,
    TrayMicrophoneRestricted,
    TrayFixEngine,
    TrayBackupFolderMissing,
    TrayBackupFolderNotWritable,
    TrayFixBackup,
    TrayProfileOnDefault,
    TrayProfileStyleMoved,
    TrayDefaultAsrNotReady,
    TrayDefaultLlmNotReady,
    TrayModelFileMissing,
    TrayFixFlow,
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
    WidgetClipboardFailed,
    WidgetMicrophoneUnavailable,
    WidgetEngineUnavailable,
    WidgetAsrFailed,
    WidgetModelMissing,
    WidgetModelCorrupt,
    WidgetNativeUnavailable,
    WidgetDegraded,
    WidgetRecordFailed,
    WidgetCopy,
    WidgetDismiss,
}

pub const ALL_MSGS: &[Msg] = &[
    Msg::TrayStatusIdle,
    Msg::TrayStatusRecording,
    Msg::TrayStatusProcessing,
    Msg::TrayStatusNoAccessibility,
    Msg::TrayStatusEngineError,
    Msg::TrayStatusPaused,
    Msg::TrayPause,
    Msg::TraySettings,
    Msg::TrayAbout,
    Msg::TrayUpdateCheck,
    Msg::TrayUpdateFound,
    Msg::TrayCopyLast,
    Msg::TrayHistory,
    Msg::TrayStatistics,
    Msg::TrayProfiles,
    Msg::TrayProfilesManage,
    Msg::TrayWidget,
    Msg::TrayWidgetPlace,
    Msg::TrayWidgetIdle,
    Msg::TrayWidgetIdleHidden,
    Msg::TrayWidgetIdleRing,
    Msg::TrayWidgetIdleLabel,
    Msg::TrayFixPermissions,
    Msg::TrayMicrophoneDenied,
    Msg::TrayMicrophoneRestricted,
    Msg::TrayFixEngine,
    Msg::TrayBackupFolderMissing,
    Msg::TrayBackupFolderNotWritable,
    Msg::TrayFixBackup,
    Msg::TrayProfileOnDefault,
    Msg::TrayProfileStyleMoved,
    Msg::TrayDefaultAsrNotReady,
    Msg::TrayDefaultLlmNotReady,
    Msg::TrayModelFileMissing,
    Msg::TrayFixFlow,
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
    Msg::WidgetClipboardFailed,
    Msg::WidgetMicrophoneUnavailable,
    Msg::WidgetEngineUnavailable,
    Msg::WidgetAsrFailed,
    Msg::WidgetModelMissing,
    Msg::WidgetModelCorrupt,
    Msg::WidgetNativeUnavailable,
    Msg::WidgetDegraded,
    Msg::WidgetRecordFailed,
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
            Msg::TrayStatusPaused => "tray.status.paused",
            Msg::TrayPause => "tray.pause",
            Msg::TraySettings => "tray.settings",
            Msg::TrayAbout => "tray.about",
            Msg::TrayUpdateCheck => "tray.update_check",
            Msg::TrayUpdateFound => "tray.update_found",
            Msg::TrayCopyLast => "tray.copy_last",
            Msg::TrayHistory => "tray.history",
            Msg::TrayStatistics => "tray.statistics",
            Msg::TrayProfiles => "tray.profiles",
            Msg::TrayProfilesManage => "tray.profiles_manage",
            Msg::TrayWidget => "tray.widget",
            Msg::TrayWidgetPlace => "tray.widget_place",
            Msg::TrayWidgetIdle => "tray.widget_idle",
            Msg::TrayWidgetIdleHidden => "tray.widget_idle_hidden",
            Msg::TrayWidgetIdleRing => "tray.widget_idle_ring",
            Msg::TrayWidgetIdleLabel => "tray.widget_idle_label",
            Msg::TrayFixPermissions => "tray.fix_permissions",
            Msg::TrayMicrophoneDenied => "tray.microphone_denied",
            Msg::TrayMicrophoneRestricted => "tray.microphone_restricted",
            Msg::TrayFixEngine => "tray.fix_engine",
            Msg::TrayBackupFolderMissing => "tray.backup_folder_missing",
            Msg::TrayBackupFolderNotWritable => "tray.backup_folder_not_writable",
            Msg::TrayFixBackup => "tray.fix_backup",
            Msg::TrayProfileOnDefault => "tray.profile_on_default",
            Msg::TrayProfileStyleMoved => "tray.profile_style_moved",
            Msg::TrayDefaultAsrNotReady => "tray.default_asr_not_ready",
            Msg::TrayDefaultLlmNotReady => "tray.default_llm_not_ready",
            Msg::TrayModelFileMissing => "tray.model_file_missing",
            Msg::TrayFixFlow => "tray.fix_flow",
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
            Msg::WidgetClipboardFailed => "widget.clipboard_failed",
            Msg::WidgetMicrophoneUnavailable => "widget.microphone_unavailable",
            Msg::WidgetEngineUnavailable => "widget.engine_unavailable",
            Msg::WidgetAsrFailed => "widget.asr_failed",
            Msg::WidgetModelMissing => "widget.model_missing",
            Msg::WidgetModelCorrupt => "widget.model_corrupt",
            Msg::WidgetNativeUnavailable => "widget.native_unavailable",
            Msg::WidgetDegraded => "widget.degraded",
            Msg::WidgetRecordFailed => "widget.record_failed",
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
        (Language::ZhTw, Msg::TrayStatusPaused) => "聽寫已暫停",
        (Language::En, Msg::TrayStatusPaused) => "Dictation paused",
        (Language::ZhTw, Msg::TrayPause) => "暫停聽寫",
        (Language::En, Msg::TrayPause) => "Pause dictation",
        (Language::ZhTw, Msg::TraySettings) => "設定...",
        (Language::En, Msg::TraySettings) => "Settings...",
        (Language::ZhTw, Msg::TrayAbout) => "關於 {app}",
        (Language::En, Msg::TrayAbout) => "About {app}",
        (Language::ZhTw, Msg::TrayUpdateCheck) => "檢查更新",
        (Language::En, Msg::TrayUpdateCheck) => "Check for Updates",
        (Language::ZhTw, Msg::TrayUpdateFound) => "有新版本：{version}",
        (Language::En, Msg::TrayUpdateFound) => "Update available: {version}",
        (Language::ZhTw, Msg::TrayCopyLast) => "複製上一次的結果",
        (Language::En, Msg::TrayCopyLast) => "Copy the last result",
        (Language::ZhTw, Msg::TrayHistory) => "歷史紀錄",
        (Language::En, Msg::TrayHistory) => "History",
        (Language::ZhTw, Msg::TrayStatistics) => "統計",
        (Language::En, Msg::TrayStatistics) => "Statistics",
        (Language::ZhTw, Msg::TrayProfiles) => "預設情境",
        (Language::En, Msg::TrayProfiles) => "Default profile",
        (Language::ZhTw, Msg::TrayProfilesManage) => "管理情境",
        (Language::En, Msg::TrayProfilesManage) => "Manage profiles",
        (Language::ZhTw, Msg::TrayWidget) => "浮動視窗",
        (Language::En, Msg::TrayWidget) => "Widget",
        (Language::ZhTw, Msg::TrayWidgetIdle) => "待命時",
        (Language::En, Msg::TrayWidgetIdle) => "While idle",
        (Language::ZhTw, Msg::TrayWidgetPlace) => "預覽浮動視窗",
        (Language::En, Msg::TrayWidgetPlace) => "Preview the widget",
        (Language::ZhTw, Msg::TrayWidgetIdleHidden) => "隱藏",
        (Language::En, Msg::TrayWidgetIdleHidden) => "Hidden",
        (Language::ZhTw, Msg::TrayWidgetIdleRing) => "圓環",
        (Language::En, Msg::TrayWidgetIdleRing) => "Ring",
        (Language::ZhTw, Msg::TrayWidgetIdleLabel) => "完整",
        (Language::En, Msg::TrayWidgetIdleLabel) => "Full",
        (Language::ZhTw, Msg::TrayFixPermissions) => "權限設定",
        (Language::En, Msg::TrayFixPermissions) => "Permissions",
        (Language::ZhTw, Msg::TrayMicrophoneDenied) => "需要麥克風權限",
        (Language::En, Msg::TrayMicrophoneDenied) => "Microphone permission required",
        (Language::ZhTw, Msg::TrayMicrophoneRestricted) => "麥克風受到這台 Mac 的管理設定限制",
        (Language::En, Msg::TrayMicrophoneRestricted) => {
            "The microphone is restricted by this Mac's management settings"
        }
        (Language::ZhTw, Msg::TrayFixEngine) => "引擎",
        (Language::En, Msg::TrayFixEngine) => "Engines",
        (Language::ZhTw, Msg::TrayBackupFolderMissing) => "找不到備份資料夾",
        (Language::En, Msg::TrayBackupFolderMissing) => "Backup folder not found",
        (Language::ZhTw, Msg::TrayBackupFolderNotWritable) => "無法寫入備份資料夾",
        (Language::En, Msg::TrayBackupFolderNotWritable) => "Backup folder cannot be written",
        (Language::ZhTw, Msg::TrayFixBackup) => "備份",
        (Language::En, Msg::TrayFixBackup) => "Backup",
        (Language::ZhTw, Msg::TrayProfileOnDefault) => {
            "情境「{name}」指定的引擎無法使用，目前使用預設引擎"
        }
        (Language::En, Msg::TrayProfileOnDefault) => {
            "Profile \"{name}\": its engine cannot run, the default is used"
        }
        (Language::ZhTw, Msg::TrayProfileStyleMoved) => "情境「{name}」的範本寫法有新版本",
        (Language::En, Msg::TrayProfileStyleMoved) => {
            "Profile \"{name}\": its template has a new writing style"
        }
        (Language::ZhTw, Msg::TrayDefaultAsrNotReady) => "預設的語音辨識引擎無法使用",
        (Language::En, Msg::TrayDefaultAsrNotReady) => "The default speech engine cannot run",
        (Language::ZhTw, Msg::TrayDefaultLlmNotReady) => "預設的文字改寫引擎無法使用",
        (Language::En, Msg::TrayDefaultLlmNotReady) => "The default rewriting engine cannot run",
        (Language::ZhTw, Msg::TrayModelFileMissing) => "找不到模型檔案：{name}",
        (Language::En, Msg::TrayModelFileMissing) => "Model file not found: {name}",
        (Language::ZhTw, Msg::TrayFixFlow) => "情境",
        (Language::En, Msg::TrayFixFlow) => "Profiles",
        (Language::ZhTw, Msg::TrayQuit) => "結束 {app}",
        (Language::En, Msg::TrayQuit) => "Quit {app}",
        (Language::ZhTw, Msg::WidgetListening) => "聆聽中",
        (Language::En, Msg::WidgetListening) => "Listening",
        (Language::ZhTw, Msg::WidgetLocked) => "再按一次快捷鍵結束",
        (Language::En, Msg::WidgetLocked) => "Press the shortcut again to finish",
        (Language::ZhTw, Msg::WidgetProcessing) => "處理中",
        (Language::En, Msg::WidgetProcessing) => "Processing",
        (Language::ZhTw, Msg::WidgetInserting) => "填入中",
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
        (Language::ZhTw, Msg::WidgetAccessibilityRequired) => "沒有輔助使用權限，文字未填入",
        (Language::En, Msg::WidgetAccessibilityRequired) => {
            "No Accessibility permission; the text was not inserted"
        }
        (Language::ZhTw, Msg::WidgetClipboardFailed) => "無法寫入剪貼簿，文字未填入",
        (Language::En, Msg::WidgetClipboardFailed) => {
            "The clipboard refused the text; it was not inserted"
        }
        (Language::ZhTw, Msg::WidgetMicrophoneUnavailable) => "無法使用麥克風",
        (Language::En, Msg::WidgetMicrophoneUnavailable) => "Microphone unavailable",
        (Language::ZhTw, Msg::WidgetEngineUnavailable) => "引擎尚未就緒",
        (Language::En, Msg::WidgetEngineUnavailable) => "Engine not ready",
        (Language::ZhTw, Msg::WidgetAsrFailed) => "語音辨識失敗，未填入文字",
        (Language::En, Msg::WidgetAsrFailed) => "Speech recognition failed; nothing was inserted",
        (Language::ZhTw, Msg::WidgetModelMissing) => "找不到本機模型",
        (Language::En, Msg::WidgetModelMissing) => "Local model not found",
        (Language::ZhTw, Msg::WidgetModelCorrupt) => "模型檔案損毀",
        (Language::En, Msg::WidgetModelCorrupt) => "Model file is damaged",
        (Language::ZhTw, Msg::WidgetNativeUnavailable) => "Apple 的服務目前無法使用",
        (Language::En, Msg::WidgetNativeUnavailable) => "The Apple service is not available",
        (Language::ZhTw, Msg::WidgetDegraded) => "改寫失敗，改用原始辨識結果",
        (Language::En, Msg::WidgetDegraded) => "Rewrite failed; the raw transcript was used",
        (Language::ZhTw, Msg::WidgetRecordFailed) => "已填入，但紀錄未保存",
        (Language::En, Msg::WidgetRecordFailed) => "Inserted, but the record was not saved",
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
