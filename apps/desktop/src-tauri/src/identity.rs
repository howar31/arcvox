//! The app's name and version as every visible surface shows them (menu-bar
//! menu, About page), so they read the same everywhere. A development build
//! is made under another product name ("Arcvox Dev") and so has its own
//! folders, keychain entries and permissions.

use tauri::PackageInfo;

/// The app name from the bundle ("Arcvox").
pub fn name(info: &PackageInfo) -> String {
    info.name.clone()
}

/// "v0.1.0": the leading v matches the release names.
pub fn version(info: &PackageInfo) -> String {
    version_text(&info.version.to_string())
}

/// Whether this is a development build, made under its own product name.
pub fn is_dev(info: &PackageInfo) -> bool {
    dev_name(&info.name)
}

fn dev_name(name: &str) -> bool {
    name.ends_with(" Dev")
}

/// "Arcvox v0.1.0".
pub fn name_and_version(info: &PackageInfo) -> String {
    format!("{} {}", name(info), version(info))
}

fn version_text(version: &str) -> String {
    format!("v{version}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_development_build_is_known_by_its_name() {
        assert!(dev_name("Arcvox Dev"));
        assert!(!dev_name("Arcvox"));
    }

    #[test]
    fn the_version_has_a_leading_v() {
        assert_eq!(version_text("0.1.0"), "v0.1.0");
    }
}
