//! Settings files, backups of the history, and the devices whose rows are here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use arcvox_core::backup::{folder_state, BackupInfo, FolderState, ImportReport, WriteReport};
use arcvox_core::history::DeviceRow;
use arcvox_core::settings_file::{Note, SettingsFile};
use arcvox_core::{BackupSource, Engine};
use serde::Serialize;
use serde_json::Value;
use tauri::State;

use super::client::apply_client;
use super::{engine, CmdError, CmdResult};
use crate::app::AppState;
use crate::backup_auto::{look, target, BackupStatus, LastWrite};
use crate::settings::{BackupCategories, ClientSettings};

/// What a settings file says about itself, for the import dialog.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsInfo {
    pub file_name: String,
    pub app_version: String,
    pub platform: String,
    pub device_name: String,
    pub exported_at: i64,
    /// The sections it has that this version knows.
    pub sections: Vec<String>,
    pub unknown: Vec<String>,
}

/// What importing a settings file did.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsResult {
    pub applied: Vec<String>,
    pub notes: Vec<Note>,
}

/// The result of `import_settings`: the client settings to apply, when the
/// file had sections of the client.
pub(crate) struct Imported {
    pub client: Option<ClientSettings>,
    pub outcome: SettingsResult,
    /// The core's files as they were, each with its copy.
    pub copies: Vec<(PathBuf, PathBuf)>,
}

/// History and recording counts, shown where a backup's content is chosen.
#[derive(Debug, Clone, Serialize)]
pub struct Sizes {
    pub rows: u64,
    pub recordings: u64,
    pub recording_bytes: u64,
}

/// A settings file with the named sections of the core and of this client.
pub(crate) fn build_settings_file(
    engine: &Engine,
    client: &ClientSettings,
    sections: &[String],
    app_version: &str,
) -> CmdResult<SettingsFile> {
    let mut file = SettingsFile::new(app_version, std::env::consts::OS, &engine.device().name);
    file.sections = engine.settings_export(sections)?;
    for (name, value) in client.export_sections(sections) {
        // `general` has a part of each side.
        match (file.sections.get_mut(&name), value) {
            (Some(Value::Object(core)), Value::Object(client)) => core.extend(client),
            (_, value) => {
                file.sections.insert(name, value);
            }
        }
    }
    Ok(file)
}

fn read_settings(path: &Path) -> CmdResult<SettingsFile> {
    Ok(SettingsFile::parse(&std::fs::read(path)?)?)
}

pub(crate) fn settings_info(path: &Path) -> CmdResult<SettingsInfo> {
    let file = read_settings(path)?;
    let unknown = file.unknown_sections();
    Ok(SettingsInfo {
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        sections: file
            .sections
            .keys()
            .filter(|k| !unknown.contains(k))
            .cloned()
            .collect(),
        unknown,
        app_version: file.app_version,
        platform: file.platform,
        device_name: file.device_name,
        exported_at: file.exported_at,
    })
}

/// The application ids among `profile_ids` and the per-application insertion
/// settings of `client` that no installed application has. Ids in `known`
/// were in the settings before and are left out.
pub(crate) fn apps_not_installed(
    profile_ids: &[String],
    client: Option<&ClientSettings>,
    known: &BTreeSet<String>,
) -> Option<Note> {
    let ids: BTreeSet<&str> = profile_ids
        .iter()
        .map(String::as_str)
        .chain(
            client
                .into_iter()
                .flat_map(|c| c.insertion.per_app.keys().map(String::as_str)),
        )
        .collect();
    let names: Vec<String> = ids
        .into_iter()
        .filter(|id| !known.contains(*id) && super::system::app_name(id).is_none())
        .map(str::to_string)
        .collect();
    (!names.is_empty()).then_some(Note::AppsNotInstalled { names })
}

/// Applies the named sections of `file`: the core's part at once, the
/// client's part as the returned settings. A file whose client sections
/// cannot be read changes nothing.
pub(crate) async fn import_settings(
    engine: &Engine,
    client: &ClientSettings,
    file: &SettingsFile,
    sections: &[String],
) -> CmdResult<Imported> {
    let client_applied = ClientSettings::applied_sections(file, sections);
    let next = client.with_sections(file, sections).map_err(|reason| {
        CmdError::with("settings_file_apply", reason.clone(), &[("reason", reason)])
    })?;
    // What the settings named before the import: only ids that are new are
    // worth a note.
    let known: BTreeSet<String> = engine
        .config()
        .current()
        .profiles
        .iter()
        .flat_map(|p| p.matcher.app_ids.iter().cloned())
        .chain(client.insertion.per_app.keys().cloned())
        .collect();
    let core = engine.settings_import(file, sections).await?;

    let mut applied = core.applied;
    applied.extend(client_applied.iter().cloned());
    let order = |name: &String| {
        arcvox_core::settings_file::ALL_SECTIONS
            .iter()
            .position(|s| s == name)
    };
    applied.sort_by_key(order);
    applied.dedup();

    let mut notes = core.notes;
    let profile_ids: Vec<String> = if applied.iter().any(|s| s == "profiles") {
        let config = engine.config().current();
        config
            .profiles
            .iter()
            .flat_map(|p| p.matcher.app_ids.iter().cloned())
            .collect()
    } else {
        Vec::new()
    };
    let insertion = client_applied.iter().any(|s| s == "insertion");
    notes.extend(apps_not_installed(
        &profile_ids,
        insertion.then_some(&next),
        &known,
    ));
    Ok(Imported {
        client: (!client_applied.is_empty()).then_some(next),
        outcome: SettingsResult { applied, notes },
        copies: core.copies,
    })
}

/// The sections of the kinds ticked for the backup folder.
pub(crate) fn snapshot_sections(cats: BackupCategories) -> Vec<String> {
    use arcvox_core::settings_file::{CONTENT_SECTIONS, SETTING_SECTIONS};
    let mut sections = Vec::new();
    if cats.settings {
        sections.extend(SETTING_SECTIONS.iter().map(|s| s.to_string()));
    }
    if cats.content {
        sections.extend(CONTENT_SECTIONS.iter().map(|s| s.to_string()));
    }
    sections
}

fn source(path: String, folder: bool) -> BackupSource {
    if folder {
        BackupSource::Folder(path.into())
    } else {
        BackupSource::File(path.into())
    }
}

#[tauri::command]
pub fn settings_export(
    state: State<'_, AppState>,
    path: String,
    sections: Vec<String>,
) -> CmdResult<()> {
    let client = ClientSettings::load(&state.dirs.config_dir);
    let file = build_settings_file(&engine(&state)?, &client, &sections, &state.version)?;
    std::fs::write(path, file.to_bytes())?;
    Ok(())
}

#[tauri::command]
pub fn settings_inspect(path: String) -> CmdResult<SettingsInfo> {
    settings_info(Path::new(&path))
}

#[tauri::command]
pub async fn settings_import(
    state: State<'_, AppState>,
    path: String,
    sections: Vec<String>,
) -> CmdResult<SettingsResult> {
    let e = engine(&state)?;
    let file = read_settings(Path::new(&path))?;
    let client = ClientSettings::load(&state.dirs.config_dir);
    let imported = import_settings(&e, &client, &file, &sections).await?;
    // Engines from the file take their models' ids and the ones of this
    // machine's models are added.
    state.sync_engines();
    if let Some(next) = imported.client {
        let applied = ClientSettings::copy_aside(&state.dirs.config_dir)
            .map_err(CmdError::from)
            .and_then(|_| apply_client(&state, next));
        if let Err(err) = applied {
            // The two parts go together: the core's part is put back.
            e.config().restore_copies(&imported.copies)?;
            return Err(err);
        }
    }
    state.backup.touch();
    Ok(imported.outcome)
}

#[tauri::command]
pub fn backup_sizes(state: State<'_, AppState>) -> CmdResult<Sizes> {
    let e = engine(&state)?;
    let s = e.history().storage(&e.recordings_dir())?;
    Ok(Sizes {
        rows: s.rows,
        recordings: s.recordings,
        recording_bytes: s.recording_bytes,
    })
}

#[tauri::command]
pub async fn backup_export(
    state: State<'_, AppState>,
    path: String,
    categories: BackupCategories,
) -> CmdResult<WriteReport> {
    let e = engine(&state)?;
    // A file of records holds records only.
    let categories = BackupCategories {
        settings: false,
        content: false,
        ..categories
    };
    Ok(
        e.backup_export_file(Path::new(&path), categories.into(), None)
            .await?,
    )
}

#[tauri::command]
pub async fn backup_inspect(
    state: State<'_, AppState>,
    path: String,
    folder: bool,
) -> CmdResult<BackupInfo> {
    Ok(engine(&state)?
        .backup_inspect(&source(path, folder))
        .await?)
}

#[tauri::command]
pub async fn backup_import(
    state: State<'_, AppState>,
    path: String,
    folder: bool,
    devices: Vec<String>,
    categories: BackupCategories,
) -> CmdResult<ImportReport> {
    let report = engine(&state)?
        .backup_import(&source(path, folder), devices, categories.into())
        .await?;
    Ok(report)
}

#[tauri::command]
pub fn backup_status(state: State<'_, AppState>) -> BackupStatus {
    state.backup.status()
}

/// Writes the backup folder as soon as no dictation is being processed;
/// the result arrives as a `backup-status` event.
#[tauri::command]
pub fn backup_run_now(state: State<'_, AppState>) {
    state.backup.run_now();
}

/// What the backup folder is right now; `probe` writes a small file to find
/// out whether it takes one. A change reaches the menu-bar icon too.
#[tauri::command]
pub async fn backup_folder_state(
    state: State<'_, AppState>,
    probe: bool,
) -> CmdResult<FolderState> {
    let chosen = ClientSettings::load(&state.dirs.config_dir).backup;
    let last = LastWrite::load(&state.dirs.config_dir).map(|l| l.folder);
    let found = look(
        chosen.folder.as_deref(),
        &state.dirs.backup_dir,
        last.as_deref(),
        probe,
    );
    Ok(state
        .backup
        .saw_with(found, probe, chosen.auto && chosen.recreate))
}

#[derive(Serialize)]
pub struct SuggestedFolder {
    path: String,
    /// The path with the home folder as `~`.
    short: String,
}

/// The folder backups go to while the user has chosen none.
#[tauri::command]
pub fn backup_suggested_folder(state: State<'_, AppState>) -> SuggestedFolder {
    let dir = &state.dirs.backup_dir;
    SuggestedFolder {
        path: dir.display().to_string(),
        short: arcvox_core::paths::tilde(dir),
    }
}

/// The folder backups go to now.
fn backup_target(state: &AppState) -> PathBuf {
    let chosen = ClientSettings::load(&state.dirs.config_dir).backup.folder;
    target(chosen.as_deref(), &state.dirs.backup_dir)
}

/// Makes the folder again and writes to it. Answers with what the folder is
/// afterwards; nothing is written unless it can be used.
#[tauri::command]
pub async fn backup_folder_create(state: State<'_, AppState>) -> CmdResult<FolderState> {
    let folder = backup_target(&state);
    if let Err(e) = std::fs::create_dir_all(&folder) {
        tracing::warn!(error = %e, "the backup folder was not made");
    }
    let now = state.backup.saw(folder_state(&folder, true), true);
    if now.usable() {
        state.backup.run_now();
    }
    Ok(now)
}

/// Opens the backup folder in the file manager.
#[tauri::command]
pub fn backup_folder_open(state: State<'_, AppState>) -> CmdResult<()> {
    let folder = backup_target(&state);
    if !folder.is_dir() {
        return Err(CmdError::with(
            "folder_missing",
            format!("the folder {} does not exist", folder.display()),
            &[("path", folder.display().to_string())],
        ));
    }
    std::process::Command::new("open").arg(folder).spawn()?;
    Ok(())
}

#[tauri::command]
pub fn devices_list(state: State<'_, AppState>) -> CmdResult<Vec<DeviceRow>> {
    Ok(engine(&state)?.devices()?)
}

#[tauri::command]
pub async fn device_rename(state: State<'_, AppState>, id: String, name: String) -> CmdResult<()> {
    engine(&state)?.rename_device(&id, &name).await?;
    state.backup.touch();
    Ok(())
}

#[tauri::command]
pub async fn device_remove(state: State<'_, AppState>, id: String) -> CmdResult<usize> {
    let removed = engine(&state)?.remove_device(&id).await?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::settings_file::ALL_SECTIONS;
    use arcvox_core::{DataDir, MemorySecretStore};
    use std::sync::Arc;

    async fn engine(root: &Path) -> Engine {
        Engine::open(
            DataDir::custom(root),
            Arc::new(MemorySecretStore::default()),
        )
        .await
        .unwrap()
    }

    fn all() -> Vec<String> {
        ALL_SECTIONS.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn a_settings_file_holds_both_parts() {
        let tmp = tempfile::tempdir().unwrap();
        let e = engine(tmp.path()).await;
        let file = build_settings_file(&e, &ClientSettings::default(), &all(), "0.1.0").unwrap();
        let mut names: Vec<_> = file.sections.keys().cloned().collect();
        names.sort();
        let mut expected = all();
        // The default pair travels with the engines.
        expected.push("default_engines".into());
        expected.sort();
        assert_eq!(names, expected);
        assert!(file.sections["general"].get("core").is_some());
        assert!(file.sections["general"].get("client").is_some());
        assert_eq!(file.app_version, "0.1.0");
        assert_eq!(file.device_name, e.device().name);

        let one = build_settings_file(
            &e,
            &ClientSettings::default(),
            &["dictionary".to_string()],
            "0.1.0",
        )
        .unwrap();
        assert_eq!(one.sections.len(), 1);
    }

    #[tokio::test]
    async fn inspect_names_the_sections_and_the_unknown_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let e = engine(tmp.path()).await;
        let mut file = build_settings_file(
            &e,
            &ClientSettings::default(),
            &["dictionary".to_string(), "insertion".to_string()],
            "0.1.0",
        )
        .unwrap();
        file.sections
            .insert("gestures".into(), serde_json::json!({}));
        let path = tmp.path().join("s.json");
        std::fs::write(&path, file.to_bytes()).unwrap();
        let info = settings_info(&path).unwrap();
        assert_eq!(
            info.sections,
            vec!["dictionary".to_string(), "insertion".to_string()]
        );
        assert_eq!(info.unknown, vec!["gestures".to_string()]);
        assert_eq!(
            (info.file_name.as_str(), info.platform.as_str()),
            ("s.json", std::env::consts::OS)
        );

        std::fs::write(&path, b"{\"format\":\"arcvox-backup\",\"version\":1}").unwrap();
        assert!(settings_info(&path).is_err());
    }

    #[tokio::test]
    async fn a_settings_file_imports_both_parts() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        a.config().update(|c| c.general.retry_count = 4).unwrap();
        let mut client = ClientSettings {
            max_utterance_secs: 99,
            ..ClientSettings::default()
        };
        client.insertion.keep_on_clipboard = true;
        let file = build_settings_file(&a, &client, &all(), "0.1.0").unwrap();

        let b = engine(&tmp.path().join("b")).await;
        let got = import_settings(&b, &ClientSettings::default(), &file, &all())
            .await
            .unwrap();
        assert_eq!(got.outcome.applied, all());
        let next = got.client.expect("the client's sections were in the file");
        assert_eq!(next.max_utterance_secs, 99);
        assert!(next.insertion.keep_on_clipboard);
        assert_eq!(b.config().current().general.retry_count, 4);
    }

    #[tokio::test]
    async fn a_file_without_client_sections_leaves_the_client_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        let file = build_settings_file(
            &a,
            &ClientSettings::default(),
            &["dictionary".to_string()],
            "0.1.0",
        )
        .unwrap();
        let b = engine(&tmp.path().join("b")).await;
        let got = import_settings(&b, &ClientSettings::default(), &file, &all())
            .await
            .unwrap();
        assert!(got.client.is_none());
        assert_eq!(got.outcome.applied, vec!["dictionary".to_string()]);
    }

    #[tokio::test]
    async fn a_bad_client_section_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let a = engine(&tmp.path().join("a")).await;
        a.config().update(|c| c.general.retry_count = 4).unwrap();
        let mut file =
            build_settings_file(&a, &ClientSettings::default(), &all(), "0.1.0").unwrap();
        file.sections
            .insert("recording".into(), serde_json::json!(5));
        let b = engine(&tmp.path().join("b")).await;
        let before = b.config().current();
        assert!(
            import_settings(&b, &ClientSettings::default(), &file, &all())
                .await
                .is_err()
        );
        assert_eq!(b.config().current(), before);
    }

    #[test]
    fn the_copy_in_the_folder_has_the_sections_of_the_kinds_ticked() {
        let cats = |settings, content| BackupCategories {
            settings,
            content,
            ..BackupCategories::default()
        };
        assert_eq!(
            snapshot_sections(cats(true, false)),
            vec!["recording", "insertion", "general", "appearance"]
        );
        assert_eq!(
            snapshot_sections(cats(false, true)),
            vec!["profiles", "providers", "dictionary", "base_prompt"]
        );
        assert_eq!(snapshot_sections(cats(true, true)).len(), 8);
        assert!(snapshot_sections(cats(false, false)).is_empty());
    }

    #[test]
    fn ids_without_an_installed_application_are_named() {
        let mut client = ClientSettings::default();
        client.insertion.per_app.insert(
            "invalid.bundle.id.zz".into(),
            crate::settings::InsertMethod::Type,
        );
        let profile_ids = vec![
            "invalid.bundle.id.yy".to_string(),
            "invalid.bundle.id.zz".to_string(),
        ];
        let none = BTreeSet::new();
        let note = apps_not_installed(&profile_ids, Some(&client), &none).unwrap();
        assert_eq!(
            note,
            Note::AppsNotInstalled {
                names: vec!["invalid.bundle.id.yy".into(), "invalid.bundle.id.zz".into()]
            }
        );
        assert_eq!(apps_not_installed(&[], None, &none), None);
        // An id the settings already had before the import is nothing new.
        let known: BTreeSet<String> = ["invalid.bundle.id.zz".to_string()].into();
        assert_eq!(
            apps_not_installed(&profile_ids, Some(&client), &known),
            Some(Note::AppsNotInstalled {
                names: vec!["invalid.bundle.id.yy".into()]
            })
        );
    }

    #[tokio::test]
    async fn importing_the_settings_this_device_exported_reports_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let e = engine(tmp.path()).await;
        e.config()
            .update(|c| c.profiles[0].matcher.app_ids = vec!["invalid.bundle.id.yy".into()])
            .unwrap();
        let mut client = ClientSettings::default();
        client.insertion.per_app.insert(
            "invalid.bundle.id.zz".into(),
            crate::settings::InsertMethod::Type,
        );
        let file = build_settings_file(&e, &client, &all(), "0.1.0").unwrap();
        let got = import_settings(&e, &client, &file, &all()).await.unwrap();
        assert_eq!(got.outcome.notes, vec![]);
    }
}
