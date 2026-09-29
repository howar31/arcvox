fn main() {
    // The target, not the host, decides; swiftc only exists on macOS hosts.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        #[cfg(target_os = "macos")]
        build_apple_bridge();
    }
    tauri_build::build()
}

/// Compiles `apple/KongweApple.swift` into a static archive and links it with
/// the Swift runtime that ships in `/usr/lib/swift`, so no Swift libraries are
/// bundled. Speech and FoundationModels are weak so the app launches on macOS 14.
#[cfg(target_os = "macos")]
fn build_apple_bridge() {
    use std::process::Command;

    let out = std::env::var("OUT_DIR").unwrap();
    let src = "apple/KongweApple.swift";
    println!("cargo:rerun-if-changed={src}");
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        _ => "x86_64",
    };
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-O",
            "-target",
            &format!("{arch}-apple-macos14.0"),
            "-module-name",
            "KongweApple",
            "-module-cache-path",
            &format!("{out}/swift-module-cache"),
            "-o",
            &format!("{out}/libkongwe_apple.a"),
            src,
        ])
        .status()
        .expect("xcrun swiftc");
    assert!(status.success(), "swiftc failed");
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=kongwe_apple");

    // The archive carries autolink entries for the Swift libraries and
    // frameworks it uses; the linker only needs the search paths.
    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("xcrun --show-sdk-path");
    let sdk = String::from_utf8(sdk.stdout).unwrap();
    println!(
        "cargo:rustc-link-search=native={}/usr/lib/swift",
        sdk.trim()
    );
    println!("cargo:rustc-link-search=native=/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,Speech");
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,FoundationModels");
}
