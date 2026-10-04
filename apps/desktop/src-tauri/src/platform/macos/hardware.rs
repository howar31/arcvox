//! What the system says about this machine: an id that stays the same for
//! its life, and the name the user gave it.

use std::process::Command;

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// The value of `"IOPlatformUUID" = "<value>"` in the registry listing.
fn platform_uuid(listing: &str) -> Option<String> {
    let line = listing.lines().find(|l| l.contains("\"IOPlatformUUID\""))?;
    let value = line.split('=').nth(1)?.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

pub fn hardware_id() -> Option<String> {
    platform_uuid(&output(
        "/usr/sbin/ioreg",
        &["-rd1", "-c", "IOPlatformExpertDevice"],
    )?)
}

pub fn computer_name() -> Option<String> {
    let name = output("/usr/sbin/scutil", &["--get", "ComputerName"])?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uuid_is_read_from_the_listing() {
        let listing = "  {\n    \"IOPlatformSerialNumber\" = \"C0\"\n    \"IOPlatformUUID\" = \"AB12-34\"\n  }";
        assert_eq!(platform_uuid(listing).as_deref(), Some("AB12-34"));
        assert_eq!(platform_uuid("nothing here"), None);
        assert_eq!(platform_uuid("\"IOPlatformUUID\" = \"\""), None);
    }

    #[test]
    fn the_id_is_stable_and_not_empty() {
        let a = hardware_id().expect("this machine has an id");
        assert!(a.len() >= 8);
        assert_eq!(hardware_id().as_deref(), Some(a.as_str()));
        assert!(computer_name().is_some());
    }
}
