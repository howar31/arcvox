//! Builds ggml once with llama.cpp (and its mtmd library), then whisper.cpp
//! against that same ggml, so the final binary holds a single copy of every
//! ggml symbol. Both are static libraries.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let vendor = manifest.join("vendor");
    let llama_src = vendor.join("llama.cpp");
    let whisper_src = vendor.join("whisper.cpp");
    let target = env::var("TARGET").unwrap();
    let apple = target.contains("apple");
    let windows = target.contains("windows");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=vendor");

    let profile = if env::var("DEBUG").as_deref() == Ok("true") {
        "RelWithDebInfo"
    } else {
        "Release"
    };
    let metal = cfg!(feature = "metal");
    let vulkan = cfg!(feature = "vulkan");
    let cuda = cfg!(feature = "cuda");
    let on = |b: bool| if b { "ON" } else { "OFF" };

    // llama.cpp, mtmd and ggml.
    let mut llama = cmake::Config::new(&llama_src);
    llama
        .out_dir(out.join("llama"))
        .profile(profile)
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("LLAMA_BUILD_COMMON", "OFF")
        .define("LLAMA_BUILD_TESTS", "OFF")
        .define("LLAMA_BUILD_TOOLS", "OFF")
        .define("LLAMA_BUILD_EXAMPLES", "OFF")
        .define("LLAMA_BUILD_SERVER", "OFF")
        .define("LLAMA_BUILD_APP", "OFF")
        .define("LLAMA_BUILD_MTMD", "ON")
        .define("LLAMA_CURL", "OFF")
        .define("LLAMA_OPENSSL", "OFF")
        .define("LLAMA_SUBPROCESS", "OFF")
        .define("LLAMA_ALL_WARNINGS", "OFF")
        .define("GGML_NATIVE", "OFF")
        .define("GGML_OPENMP", "OFF")
        .define("GGML_BACKEND_DL", "OFF")
        .define("GGML_METAL", on(metal))
        .define("GGML_METAL_EMBED_LIBRARY", on(metal))
        .define("GGML_VULKAN", on(vulkan))
        .define("GGML_CUDA", on(cuda))
        .pic(true);
    if apple {
        // Accelerate stays on through GGML_ACCELERATE; the BLAS backend is not used.
        llama.define("GGML_BLAS", "OFF");
    }
    if windows {
        llama.cxxflag("/utf-8");
    }
    let llama_dst = llama.build();
    let ggml_cmake = ["lib", "lib64"]
        .iter()
        .map(|d| llama_dst.join(d).join("cmake").join("ggml"))
        .find(|p| p.is_dir())
        .expect("llama.cpp installs the ggml CMake package");

    // whisper.cpp writes into its own source tree while configuring, so it is
    // configured from a copy.
    let whisper_copy = out.join("whisper-src");
    copy_dir(&whisper_src, &whisper_copy);
    let mut whisper = cmake::Config::new(&whisper_copy);
    whisper
        .out_dir(out.join("whisper"))
        .profile(profile)
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("WHISPER_BUILD_TESTS", "OFF")
        .define("WHISPER_BUILD_EXAMPLES", "OFF")
        .define("WHISPER_BUILD_SERVER", "OFF")
        .define("WHISPER_ALL_WARNINGS", "OFF")
        .define("WHISPER_ALL_WARNINGS_3RD_PARTY", "OFF")
        .define("WHISPER_USE_SYSTEM_GGML", "ON")
        .define("ggml_DIR", &ggml_cmake)
        .pic(true);
    // whisper.cpp reads these to pick the backends compiled into the shared ggml.
    if metal {
        whisper.cxxflag("-DGGML_USE_METAL");
    }
    if cuda {
        whisper.cxxflag("-DGGML_USE_CUDA");
    }
    if windows {
        whisper.cxxflag("/utf-8");
    }
    let whisper_dst = whisper.build();

    for dst in [&llama_dst, &whisper_dst] {
        for d in ["lib", "lib64"] {
            println!("cargo:rustc-link-search=native={}", dst.join(d).display());
        }
    }
    let mut libs = vec!["whisper", "mtmd", "llama", "ggml"];
    if metal {
        libs.push("ggml-metal");
    }
    if vulkan {
        libs.push("ggml-vulkan");
    }
    if cuda {
        libs.push("ggml-cuda");
    }
    libs.extend(["ggml-cpu", "ggml-base"]);
    for lib in libs {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    if apple {
        for fw in ["Foundation", "Accelerate"] {
            println!("cargo:rustc-link-lib=framework={fw}");
        }
        if metal {
            println!("cargo:rustc-link-lib=framework=Metal");
            println!("cargo:rustc-link-lib=framework=MetalKit");
        }
        println!("cargo:rustc-link-lib=c++");
    } else if windows {
        println!("cargo:rustc-link-lib=advapi32");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", llama_src.join("include").display()))
        .clang_arg(format!("-I{}", llama_src.join("ggml/include").display()))
        .clang_arg(format!("-I{}", llama_src.join("tools/mtmd").display()))
        .clang_arg(format!("-I{}", whisper_src.join("include").display()))
        .allowlist_function("(llama|mtmd|whisper)_.*")
        .allowlist_type("(llama|mtmd|whisper)_.*")
        .allowlist_type("ggml_log_level")
        .allowlist_var("(LLAMA|MTMD|WHISPER)_.*")
        .prepend_enum_name(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("bindings generated from the vendored headers");
    bindings
        .write_to_file(out.join("bindings.rs"))
        .expect("bindings written");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), &dest).unwrap();
        }
    }
}
