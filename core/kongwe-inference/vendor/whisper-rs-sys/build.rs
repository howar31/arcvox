#![allow(clippy::uninlined_format_args)]

//! Builds whisper.cpp against the ggml that llama-cpp-sys-2 builds and links,
//! so the final binary contains a single ggml.

extern crate bindgen;
extern crate semver;

use cmake::Config;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

fn main() {
    if cfg!(feature = "openblas") || cfg!(feature = "intel-sycl") {
        panic!("the openblas and intel-sycl features need a ggml built with them; the shared ggml from llama-cpp-sys-2 does not provide them");
    }

    // Exported by llama-cpp-sys-2 (`links = "llama"`): its install prefix and ggml's CMake package dir.
    let llama_root = PathBuf::from(
        env::var("DEP_LLAMA_ROOT").expect("DEP_LLAMA_ROOT is set by llama-cpp-sys-2"),
    );
    let ggml_cmake_dir = PathBuf::from(
        env::var("DEP_LLAMA_GGML_CMAKE_DIR").expect("DEP_LLAMA_GGML_CMAKE_DIR is set by llama-cpp-sys-2"),
    );
    let ggml_include = llama_root.join("include");
    assert!(
        ggml_include.join("ggml.h").is_file(),
        "ggml.h not found in {}",
        ggml_include.display()
    );

    let target = env::var("TARGET").unwrap();
    if let Some(cpp_stdlib) = get_cpp_link_stdlib(&target) {
        println!("cargo:rustc-link-lib=dylib={}", cpp_stdlib);
    }
    if target.contains("apple") {
        #[cfg(feature = "coreml")]
        {
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=CoreML");
        }
    }

    #[cfg(feature = "coreml")]
    println!("cargo:rustc-link-lib=static=whisper.coreml");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=whisper.cpp");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let whisper_root = out.join("whisper.cpp");

    // Copied on every run so edits to the vendored sources reach the build.
    let copy_options = fs_extra::dir::CopyOptions {
        overwrite: true,
        ..Default::default()
    };
    fs_extra::dir::copy("./whisper.cpp", &out, &copy_options).unwrap_or_else(|e| {
        panic!(
            "Failed to copy whisper sources into {}: {}",
            whisper_root.display(),
            e
        )
    });

    // https://github.com/rust-lang/rust-bindgen/issues/2691
    let package_msrv = match option_env!("CARGO_PKG_RUST_VERSION") {
        Some(v) if !v.is_empty() => {
            let v = semver::Version::parse(v).expect("Invalid CARGO_PKG_RUST_VERSION");
            bindgen::RustTarget::stable(v.minor, v.patch)
        }
        _ => bindgen::RustTarget::stable(88, 0),
    }
    .map_err(|v| v.to_string())
    .unwrap();

    let bindings = bindgen::Builder::default()
        .rust_edition(bindgen::RustEdition::Edition2021)
        .rust_target(package_msrv)
        .header("wrapper.h");

    #[cfg(feature = "metal")]
    let bindings = bindings.header(ggml_include.join("ggml-metal.h").display().to_string());
    #[cfg(feature = "vulkan")]
    let bindings = bindings.clang_arg("-DGGML_USE_VULKAN=1");

    // No bundled fallback: bindings must match the ggml headers actually linked.
    bindings
        .clang_arg("-I./whisper.cpp/")
        .clang_arg("-I./whisper.cpp/include")
        .clang_arg(format!("-I{}", ggml_include.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(out.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    // stop if we're on docs.rs
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let mut config = Config::new(&whisper_root);

    config
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("WHISPER_ALL_WARNINGS", "OFF")
        .define("WHISPER_ALL_WARNINGS_3RD_PARTY", "OFF")
        .define("WHISPER_BUILD_TESTS", "OFF")
        .define("WHISPER_BUILD_EXAMPLES", "OFF")
        .define("WHISPER_USE_SYSTEM_GGML", "ON")
        .define("ggml_DIR", ggml_cmake_dir.join("ggml"))
        .very_verbose(true)
        .pic(true);

    if cfg!(target_os = "windows") {
        config.cxxflag("/utf-8");
        println!("cargo:rustc-link-lib=advapi32");
    }

    if cfg!(feature = "coreml") {
        config.define("WHISPER_COREML", "ON");
        config.define("WHISPER_COREML_ALLOW_FALLBACK", "1");
    }

    // whisper.cpp reads these for backend-specific KV cache padding; the in-tree
    // ggml target used to propagate them. They follow the linked ggml, which can
    // enable a backend without this crate's feature (llama-cpp-2 turns Metal on
    // for every Apple Silicon macOS build).
    let ggml_config = std::fs::read_to_string(ggml_cmake_dir.join("ggml/ggml-config.cmake"))
        .expect("ggml-config.cmake exported by llama-cpp-sys-2");
    for backend in ["METAL", "CUDA"] {
        if ggml_config.contains(&format!("set(GGML_{backend} \"ON\")")) {
            config.cxxflag(format!("-DGGML_USE_{backend}"));
        }
    }

    if cfg!(debug_assertions) || cfg!(feature = "force-debug") {
        // debug builds are too slow to even remotely be usable,
        // so we build with optimizations even in debug mode
        config.define("CMAKE_BUILD_TYPE", "RelWithDebInfo");
        config.cxxflag("-DWHISPER_DEBUG");
    } else {
        config.define("CMAKE_BUILD_TYPE", "Release");
    }

    // Allow passing any WHISPER or CMAKE compile flags
    for (key, value) in env::vars() {
        let is_whisper_flag = key.starts_with("WHISPER_") && key != "WHISPER_DONT_GENERATE_BINDINGS";
        let is_cmake_flag = key.starts_with("CMAKE_");
        if is_whisper_flag || is_cmake_flag {
            config.define(&key, &value);
        }
    }

    let destination = config.build();

    add_link_search_path(&out.join("build")).unwrap();

    println!("cargo:rustc-link-search=native={}", destination.display());
    // ggml itself is linked from llama-cpp-sys-2's archives.
    println!("cargo:rustc-link-lib=static=whisper");

    println!(
        "cargo:WHISPER_CPP_VERSION={}",
        get_whisper_cpp_version(&whisper_root)
            .expect("Failed to read whisper.cpp CMake config")
            .expect("Could not find whisper.cpp version declaration"),
    );
}

// From https://github.com/alexcrichton/cc-rs/blob/fba7feded71ee4f63cfe885673ead6d7b4f2f454/src/lib.rs#L2462
fn get_cpp_link_stdlib(target: &str) -> Option<&'static str> {
    if target.contains("msvc") {
        None
    } else if target.contains("apple") || target.contains("freebsd") || target.contains("openbsd") {
        Some("c++")
    } else if target.contains("android") {
        Some("c++_shared")
    } else {
        Some("stdc++")
    }
}

fn add_link_search_path(dir: &std::path::Path) -> std::io::Result<()> {
    if dir.is_dir() {
        println!("cargo:rustc-link-search={}", dir.display());
        for entry in std::fs::read_dir(dir)? {
            add_link_search_path(&entry?.path())?;
        }
    }
    Ok(())
}

fn get_whisper_cpp_version(whisper_root: &std::path::Path) -> std::io::Result<Option<String>> {
    let cmake_lists = BufReader::new(File::open(whisper_root.join("CMakeLists.txt"))?);

    for line in cmake_lists.lines() {
        let line = line?;

        if let Some(suffix) = line.strip_prefix(r#"project("whisper.cpp" VERSION "#) {
            let whisper_cpp_version = suffix.trim_end_matches(')');
            return Ok(Some(whisper_cpp_version.into()));
        }
    }

    Ok(None)
}
