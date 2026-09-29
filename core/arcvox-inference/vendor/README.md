# Vendored crates

## whisper-rs-sys

A modified copy of [whisper-rs-sys](https://codeberg.org/tazz4843/whisper-rs) 0.15.0 (Unlicense),
which bundles [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (MIT, see `whisper-rs-sys/whisper.cpp/LICENSE`).
The workspace uses it through `[patch.crates-io]` in the root `Cargo.toml`.

Both whisper.cpp and llama.cpp statically link ggml, and every ggml symbol has the same name in the two
copies. Linking both copies into one binary makes each call resolve to either copy, which crashes when the
versions differ. This copy builds whisper.cpp against the ggml that `llama-cpp-sys-2` builds, so the
binary contains a single ggml.

Changes from upstream:

- `whisper.cpp/ggml/` is removed. `whisper.cpp` keeps only `src/`, `include/`, `cmake/`, `CMakeLists.txt`,
  `LICENSE` and the two `bindings/` files its CMake configuration reads.
- `src/bindings.rs` (pre-generated bindings) is removed; bindings are always generated from the ggml headers
  that are linked.
- `build.rs` configures whisper.cpp with `WHISPER_USE_SYSTEM_GGML=ON`, pointing `ggml_DIR` at the CMake
  package exported by `llama-cpp-sys-2` (`DEP_LLAMA_GGML_CMAKE_DIR`) and the include path at
  `DEP_LLAMA_ROOT/include`. It emits no link directives for ggml, defines `GGML_USE_METAL` / `GGML_USE_CUDA`
  for whisper.cpp when the exported `ggml-config.cmake` has `GGML_METAL` / `GGML_CUDA` on (so the defines
  match the linked ggml whatever features are set), and rejects the `openblas` and `intel-sycl` features.
- `Cargo.toml` depends on `llama-cpp-sys-2 = "=0.1.157"`, and the `metal`, `vulkan`, `cuda`, `hipblas` and
  `openmp` features forward to it.

Upgrading: whisper.cpp is compiled against the ggml headers of the pinned `llama-cpp-sys-2`, and
`whisper-rs` calls the bindings this copy generates. Both `whisper-rs` and `llama-cpp-2` are pinned exactly
in `core/arcvox-inference/Cargo.toml`; move them together with this copy, and rebuild and run the
real-model tests for both engines in one binary before changing any of them.
