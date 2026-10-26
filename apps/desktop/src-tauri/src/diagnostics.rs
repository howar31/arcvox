//! The report page's snapshot: the facts about this copy, the computer and
//! the settings that a bug report needs, in English and in a fixed form.
//! Nothing the user wrote or named goes in: no dictated text, no messages,
//! no paths, no profile, device, display or app names, no service
//! addresses or keys. Engines are named by their catalogue model only.

use arcvox_core::config::{Config, ProviderKind, Runtime, StepSpec};
use arcvox_core::history::{LastFailure, Outcomes};
use arcvox_core::types::ProviderId;

use crate::commands::system::Permissions;
use crate::platform::MicAccess;
use crate::settings::{ClientSettings, MicrophoneChoice, UiLanguage, WidgetIdle, WidgetSize};

/// Facts about this copy and the computer.
pub struct Machine {
    pub version: String,
    pub dev: bool,
    pub os: Option<String>,
    pub os_build: Option<String>,
    pub arch: &'static str,
    pub chip: Option<String>,
    pub memory_bytes: u64,
    /// The interface language in use, as `Language::tag`.
    pub interface: &'static str,
    pub system_language: Option<String>,
    /// Where the app runs from: `/Applications`, `~/Applications` or `other`.
    pub install: &'static str,
}

/// What the app finds at the moment of the report.
pub struct Live {
    pub permissions: Permissions,
    /// For a chosen microphone: whether it is connected.
    pub microphone_connected: Option<bool>,
    pub displays: usize,
    /// The code of the engine's failure to open, when it did not open.
    pub engine_error: Option<Option<&'static str>>,
    /// The tray's problems, by kind.
    pub problems: Vec<&'static str>,
    /// The backup folder's state, as `folder_state_name`.
    pub backup_folder: &'static str,
    /// The last seven days, and the newest failure.
    pub outcomes: Option<Outcomes>,
    pub last_failure: Option<LastFailure>,
    pub now: i64,
}

pub fn report(m: &Machine, live: &Live, config: &Config, client: &ClientSettings) -> String {
    let g = &config.general;
    let mut out: Vec<String> = Vec::new();
    out.push(format!("Arcvox {}, {}", version(m), system(m)));
    out.push(format!(
        "Chip: {}, memory {} GB",
        m.chip.as_deref().unwrap_or("unknown"),
        (m.memory_bytes as f64 / (1u64 << 30) as f64).round()
    ));
    let setting = match client.ui_language {
        UiLanguage::System => "system",
        UiLanguage::ZhTw => "zh-tw",
        UiLanguage::En => "en",
    };
    out.push(format!(
        "Language: interface={setting} ({}), system={}",
        m.interface,
        m.system_language.as_deref().unwrap_or("unknown")
    ));
    out.push(format!("Install: {}", m.install));
    let p = &live.permissions;
    out.push(format!(
        "Permissions: microphone={}, accessibility={}, keyboard={}",
        match p.microphone {
            MicAccess::Granted => "granted",
            MicAccess::NotAsked => "not asked",
            MicAccess::Denied => "denied",
            MicAccess::Restricted => "restricted",
        },
        if p.accessibility {
            "granted"
        } else {
            "not granted"
        },
        if p.keyboard_watch_only {
            "watch only"
        } else {
            "full"
        }
    ));
    let h = &client.hotkey;
    let shortcuts: Vec<String> = h
        .shortcuts
        .iter()
        .map(|s| {
            s.keys()
                .iter()
                .map(|k| k.name())
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect();
    out.push(format!(
        "Shortcuts: {} | hold {} ms, double tap {} ms, Esc cancels processing={}",
        if shortcuts.is_empty() {
            "none".to_string()
        } else {
            shortcuts.join("; ")
        },
        h.hold_threshold_ms,
        h.double_tap_window_ms,
        on(h.escape_cancels_processing)
    ));
    out.push(format!(
        "Microphone: {}",
        match client.microphone_choice() {
            MicrophoneChoice::BuiltIn => "built-in".to_string(),
            MicrophoneChoice::SystemDefault => "system default".to_string(),
            MicrophoneChoice::Named(_) => match live.microphone_connected {
                Some(false) => "chosen device, not connected".to_string(),
                _ => "chosen device".to_string(),
            },
        }
    ));
    out.push(format!(
        "Recording: max {} s, auto stop={}, transcript shown={}",
        client.max_utterance_secs,
        if client.auto_stop.enabled {
            format!("after {} s", client.auto_stop.silence_secs)
        } else {
            "off".to_string()
        },
        on(client.show_transcript)
    ));
    let c = &client.context;
    out.push(format!(
        "Context: app={}, window title={}, url={}, selected text={}, clipboard={}, limit {} bytes",
        on(c.app_name),
        on(c.window_title),
        on(c.url),
        on(c.selected_text),
        on(c.clipboard),
        g.context_limit_bytes
    ));
    out.push(format!(
        "Insertion: {}, keep on clipboard={}, per-app {}",
        client.insertion.default.name(),
        on(client.insertion.keep_on_clipboard),
        client.insertion.per_app.len()
    ));
    out.push(format!(
        "Widget: {}, idle={}, fade={}, display={}, displays {}",
        widget_size(client.widget_size),
        widget_idle(client.widget_idle),
        on(client.widget_fade.enabled),
        if client.widget_display.is_some() {
            "fixed"
        } else {
            "under pointer"
        },
        live.displays
    ));
    out.push(format!(
        "Default engines: asr={}, llm={}",
        engine(config, &config.default_engines.asr),
        engine(config, &config.default_engines.llm)
    ));
    out.push(engines_line(config));
    out.push(match live.engine_error {
        None => "Engine: open".to_string(),
        Some(code) => format!("Engine: failed to open ({})", code.unwrap_or("no code")),
    });
    let per_app = config
        .profiles
        .iter()
        .filter(|p| !p.matcher.app_ids.is_empty() || !p.matcher.url_patterns.is_empty())
        .count();
    out.push(format!(
        "Profiles: {}, per-app {per_app}",
        config.profiles.len()
    ));
    for p in &config.profiles {
        let follow = |id: &Option<ProviderId>| match id {
            Some(id) => engine(config, id),
            None => "default".to_string(),
        };
        let steps: Vec<String> = p.steps.iter().map(step).collect();
        let mut marks = Vec::new();
        if p.is_default {
            marks.push("default");
        }
        if !p.matcher.app_ids.is_empty() || !p.matcher.url_patterns.is_empty() {
            marks.push("per-app");
        }
        out.push(format!(
            "  {}{}: asr={}, llm={}, steps={}",
            profile_kind(p.preset.as_deref()),
            if marks.is_empty() {
                String::new()
            } else {
                format!(" ({})", marks.join(", "))
            },
            follow(&p.asr_provider),
            follow(&p.llm_provider),
            steps.join(" ")
        ));
    }
    out.push(format!(
        "Processing: asr timeout {} s, llm timeout {} s, retry {}, preload={}, unload after {} s, temperature {}, max tokens {}",
        g.asr_timeout_secs,
        g.llm_timeout_secs,
        g.retry_count,
        on(g.llm_preload_on_record),
        g.llm_idle_unload_secs,
        g.llm_temperature,
        g.llm_max_tokens.map_or("model".to_string(), |n| n.to_string())
    ));
    out.push(format!(
        "Keep: history {}, stages {}, recordings {}, stats={}",
        days(g.keep_history, g.history_retention_days),
        days(g.keep_stages, g.stage_retention_days),
        days(g.keeps_recordings(), g.recording_retention_days),
        on(g.keep_stats)
    ));
    out.push(format!(
        "Learning: read-back={} | log read-back={}",
        on(client.learning.read_back),
        on(client.dev.log_read_back)
    ));
    out.push(format!(
        "Folders: recordings={}, models={}, backup={} ({}), auto backup={}",
        if g.recordings_dir.is_some() {
            "custom"
        } else {
            "default"
        },
        if g.models_dir.is_some() {
            "custom"
        } else {
            "default"
        },
        if client.backup.folder.is_some() {
            "custom"
        } else {
            "default"
        },
        live.backup_folder,
        on(client.backup.auto)
    ));
    out.push(format!(
        "Problems: {}",
        if live.problems.is_empty() {
            "none".to_string()
        } else {
            live.problems.join(", ")
        }
    ));
    if let Some(o) = &live.outcomes {
        let failed: u64 = o.failed.iter().map(|(_, n)| n).sum();
        let kinds: Vec<String> = o
            .failed
            .iter()
            .map(|(code, n)| format!("{code} {n}"))
            .collect();
        out.push(format!(
            "Last 7 days: {} dictations, {failed} failed{}, {} cancelled, {} not rewritten",
            o.rows,
            if kinds.is_empty() {
                String::new()
            } else {
                format!(" ({})", kinds.join(", "))
            },
            o.cancelled,
            o.not_rewritten
        ));
    }
    out.push(match &live.last_failure {
        None => "Last failure: none".to_string(),
        Some(f) => {
            let profile = config
                .profiles
                .iter()
                .find(|p| p.id == f.profile_id)
                .map_or("removed".to_string(), |p| profile_kind(p.preset.as_deref()));
            let audio = f.audio_ms.map_or(String::new(), |ms| {
                format!(", audio {:.1} s", ms as f64 / 1000.0)
            });
            format!(
                "Last failure: {}, {}, profile={profile}{audio}",
                ago(live.now - f.ts),
                f.code
            )
        }
    });
    out.join("\n")
}

/// The bug form's fields the app fills, by their ids in the form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IssueFields {
    pub version: String,
    pub system: String,
    pub engines: String,
    pub diagnostics: String,
}

pub fn issue_fields(m: &Machine, config: &Config, diagnostics: String) -> IssueFields {
    IssueFields {
        version: version(m),
        system: system(m),
        engines: profile_engines(config),
        diagnostics,
    }
}

fn version(m: &Machine) -> String {
    format!("{}{}", m.version, if m.dev { " dev" } else { "" })
}

fn system(m: &Machine) -> String {
    let os = match (&m.os, &m.os_build) {
        (Some(v), Some(b)) => format!("macOS {v} ({b})"),
        (Some(v), None) => format!("macOS {v}"),
        _ => "macOS".to_string(),
    };
    format!("{os}, {}", m.arch)
}

/// The engines the default profile runs with, the default pair filled in.
fn profile_engines(config: &Config) -> String {
    let Some(p) = config
        .profiles
        .iter()
        .find(|p| p.is_default)
        .or(config.profiles.first())
    else {
        return "no profile".to_string();
    };
    let asr = p
        .asr_provider
        .as_ref()
        .unwrap_or(&config.default_engines.asr);
    let llm = if p
        .steps
        .iter()
        .any(|s| matches!(s, StepSpec::Llm { off: false }))
    {
        engine(
            config,
            p.llm_provider
                .as_ref()
                .unwrap_or(&config.default_engines.llm),
        )
    } else {
        "off".to_string()
    };
    format!(
        "asr={}, llm={llm} (default profile: {})",
        engine(config, asr),
        profile_kind(p.preset.as_deref())
    )
}

fn on(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

fn days(kept: bool, n: u32) -> String {
    match (kept, n) {
        (false, _) => "off".to_string(),
        (true, 0) => "forever".to_string(),
        (true, n) => format!("{n} d"),
    }
}

fn ago(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        0..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

fn widget_size(s: WidgetSize) -> &'static str {
    match s {
        WidgetSize::XSmall => "x-small",
        WidgetSize::Small => "small",
        WidgetSize::Medium => "medium",
        WidgetSize::Large => "large",
        WidgetSize::XLarge => "x-large",
    }
}

fn widget_idle(i: WidgetIdle) -> &'static str {
    i.name()
}

/// A profile by the template it was made from; the names are the user's.
fn profile_kind(preset: Option<&str>) -> String {
    preset.unwrap_or("own").to_string()
}

fn step(s: &StepSpec) -> String {
    let name = match s {
        StepSpec::Asr => "asr".to_string(),
        StepSpec::OpenCc { config, .. } => format!("opencc({config})"),
        StepSpec::Llm { .. } => "llm".to_string(),
        StepSpec::Rules { .. } => "rules".to_string(),
        StepSpec::Translate { .. } => "translate".to_string(),
    };
    if s.is_off() {
        format!("{name}(off)")
    } else {
        name
    }
}

/// An engine by its catalogue model, or by its kind where the user named it.
fn engine(config: &Config, id: &ProviderId) -> String {
    let Some(p) = config.providers.iter().find(|p| &p.id == id) else {
        return "missing".to_string();
    };
    match p.kind {
        ProviderKind::Embedded => match (&p.model_path, &p.model_id) {
            (None, Some(m)) if !m.starts_with("custom/") => m.clone(),
            _ => format!(
                "custom {}",
                match p.runtime {
                    Some(Runtime::Whisper) => "whisper",
                    Some(Runtime::Llama) => "llama",
                    None => "model",
                }
            ),
        },
        ProviderKind::Native => p.native_id.clone().unwrap_or_else(|| "native".to_string()),
        ProviderKind::Http => "network".to_string(),
    }
}

fn engines_line(config: &Config) -> String {
    let (mut catalogue, mut custom, mut native, mut network) = (0, 0, 0, 0);
    for p in &config.providers {
        match p.kind {
            ProviderKind::Embedded if engine(config, &p.id).starts_with("custom ") => custom += 1,
            ProviderKind::Embedded => catalogue += 1,
            ProviderKind::Native => native += 1,
            ProviderKind::Http => network += 1,
        }
    }
    format!(
        "Engines: local {} (catalogue {catalogue}, custom {custom}), apple {native}, network {network}",
        catalogue + custom
    )
}

/// The backup folder's state in the report.
pub fn folder_state_name(s: arcvox_core::backup::FolderState) -> &'static str {
    use arcvox_core::backup::FolderState;
    match s {
        FolderState::Usable => "usable",
        FolderState::Missing => "missing",
        FolderState::NotWritable => "not writable",
        FolderState::Blocked => "blocked",
        FolderState::Unmade => "not made yet",
    }
}

/// Where the app runs from, without the user's home folder.
pub fn install_place() -> &'static str {
    let Ok(exe) = std::env::current_exe() else {
        return "other";
    };
    if exe.starts_with("/Applications") {
        return "/Applications";
    }
    match std::env::var_os("HOME") {
        Some(home) if exe.starts_with(std::path::Path::new(&home).join("Applications")) => {
            "~/Applications"
        }
        _ => "other",
    }
}

/// A string the system reports by name; `None` when it has none.
pub fn sysctl_string(name: &std::ffi::CStr) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let mut len: usize = 0;
        // SAFETY: a null buffer asks only for the length.
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        // SAFETY: `buf` has `len` bytes of room, which `len` says.
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                buf.as_mut_ptr() as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        buf.truncate(len);
        let text = String::from_utf8_lossy(&buf)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        (!text.is_empty()).then_some(text)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::config::{Profile, ProviderConfig};
    use arcvox_core::types::ProfileId;

    fn machine() -> Machine {
        Machine {
            version: "0.4.0".into(),
            dev: false,
            os: Some("26.0".into()),
            os_build: Some("25A354".into()),
            arch: "aarch64",
            chip: Some("Apple M4".into()),
            memory_bytes: 16 << 30,
            interface: "zh-TW",
            system_language: Some("zh-Hant-TW".into()),
            install: "/Applications",
        }
    }

    fn live() -> Live {
        Live {
            permissions: Permissions {
                accessibility: true,
                keyboard_watch_only: false,
                microphone: MicAccess::Granted,
            },
            microphone_connected: Some(true),
            displays: 2,
            engine_error: None,
            problems: vec!["profile_on_default"],
            backup_folder: "usable",
            outcomes: Some(Outcomes {
                rows: 142,
                cancelled: 1,
                not_rewritten: 0,
                failed: vec![("provider_timeout".into(), 2), ("model_missing".into(), 1)],
            }),
            last_failure: Some(LastFailure {
                ts: 1_000,
                code: "provider_timeout".into(),
                profile_id: ProfileId("secret-profile".into()),
                audio_ms: Some(4200),
            }),
            now: 1_000 + 2 * 3600 + 5,
        }
    }

    /// A configuration full of what the user wrote and named.
    fn private_config() -> Config {
        let mut c = Config::http_test_config();
        let mut net = c.providers[1].clone();
        net.id = ProviderId("my-office-server".into());
        net.label = "Office GPU box".into();
        net.base_url = Some("http://10.1.2.3:8080/v1".parse().unwrap());
        net.model = Some("private-finetune".into());
        net.api_key_ref = Some("keychain-entry".into());
        c.providers.push(net);
        let mut custom = ProviderConfig::factory_engine(arcvox_core::config::EngineRole::Llm);
        custom.id = ProviderId("custom/AlexDiary.gguf".into());
        custom.model_id = Some("custom/AlexDiary.gguf".into());
        custom.runtime = Some(Runtime::Llama);
        c.providers.push(custom);
        let mut p: Profile = c.profiles[0].clone();
        p.id = ProfileId("secret-profile".into());
        p.name = "Letters to my doctor".into();
        p.is_default = false;
        p.preset = None;
        p.prompt_addition = "Sign as Dr. Wang".into();
        p.matcher.app_ids = vec!["com.example.secretapp".into()];
        p.matcher.url_patterns = vec!["intranet.example.com".into()];
        p.llm_provider = Some(ProviderId("custom/AlexDiary.gguf".into()));
        p.asr_provider = Some(ProviderId("my-office-server".into()));
        p.steps.push(StepSpec::Translate {
            off: false,
            target: "Formal Japanese for my boss".into(),
            addition: "Mention Project Falcon".into(),
            provider: None,
        });
        c.profiles.push(p);
        c.general.recordings_dir = Some("/Volumes/AlexDrive/Secret Recordings".into());
        c
    }

    fn private_client() -> ClientSettings {
        let mut s = ClientSettings {
            microphone: "Alex's AirPods Pro".into(),
            widget_display: Some("DELL U2723QE (Alex desk)".into()),
            ..Default::default()
        };
        s.insertion.per_app.insert(
            "com.example.secretapp".into(),
            crate::settings::InsertMethod::Type,
        );
        s.backup.folder = Some("/Volumes/AlexDrive/Backups".into());
        s
    }

    #[test]
    fn the_report_carries_nothing_the_user_wrote_or_named() {
        let text = report(&machine(), &live(), &private_config(), &private_client());
        for private in [
            "my-office-server",
            "Office GPU box",
            "10.1.2.3",
            "private-finetune",
            "keychain",
            "AlexDiary",
            "secret-profile",
            "Letters",
            "Dr. Wang",
            "secretapp",
            "intranet",
            "Japanese",
            "Falcon",
            "/Users",
            "Alex",
            "AirPods",
            "DELL",
            "/Volumes",
        ] {
            assert!(!text.contains(private), "{private} in:\n{text}");
        }
    }

    #[test]
    fn the_report_names_engines_by_model_and_kind() {
        let text = report(&machine(), &live(), &private_config(), &private_client());
        assert!(
            text.starts_with(
                "Arcvox 0.4.0, macOS 26.0 (25A354), aarch64\nChip: Apple M4, memory 16 GB\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("Engines: local 1 (catalogue 0, custom 1), apple 0, network 3"),
            "{text}"
        );
        assert!(
            text.contains("  own (per-app): asr=network, llm=custom llama, steps="),
            "{text}"
        );
        assert!(text.contains(" rules translate\n"), "{text}");
        assert!(
            text.contains("Shortcuts: Fn; ControlRight; AltRight; ShiftRight | hold 250 ms"),
            "{text}"
        );
        assert!(text.contains("Microphone: chosen device\n"), "{text}");
        assert!(text.contains("Folders: recordings=custom, models=default, backup=custom (usable), auto backup=off"), "{text}");
        assert!(text.contains("Last 7 days: 142 dictations, 3 failed (provider_timeout 2, model_missing 1), 1 cancelled, 0 not rewritten"), "{text}");
        assert!(
            text.ends_with("Last failure: 2 h ago, provider_timeout, profile=own, audio 4.2 s"),
            "{text}"
        );
    }

    #[test]
    fn the_form_fields_carry_version_system_and_the_default_profiles_engines() {
        let mut c = private_config();
        c.profiles[0].asr_provider = None;
        c.profiles[0].llm_provider = None;
        let f = issue_fields(&machine(), &c, "text".into());
        assert_eq!(f.version, "0.4.0");
        assert_eq!(f.system, "macOS 26.0 (25A354), aarch64");
        assert_eq!(
            f.engines,
            "asr=network, llm=network (default profile: casual)"
        );
        for p in &mut c.profiles {
            p.is_default = p.id.0 == "secret-profile";
        }
        let f = issue_fields(&machine(), &c, String::new());
        assert_eq!(
            f.engines,
            "asr=network, llm=custom llama (default profile: own)"
        );
    }

    #[test]
    fn ages_read_in_minutes_hours_and_days() {
        assert_eq!(ago(59), "0 min ago");
        assert_eq!(ago(3599), "59 min ago");
        assert_eq!(ago(7200), "2 h ago");
        assert_eq!(ago(3 * 86_400), "3 d ago");
        assert_eq!(ago(-5), "0 min ago");
    }
}
