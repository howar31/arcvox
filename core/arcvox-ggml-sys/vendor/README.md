# Vendored sources

Official release sources from [ggml-org](https://github.com/ggml-org), trimmed to what the build needs.
`build.rs` builds ggml once with llama.cpp, then whisper.cpp against that ggml, so the binary holds a
single copy of every ggml symbol.

| Directory | Source | Release | Archive sha256 | Licence |
|---|---|---|---|---|
| `llama.cpp/` | https://github.com/ggml-org/llama.cpp | `b12076` (36ee3ca) | `b8b8396c82619e68e988289d24b7ce109fe9f42b562d0e999301a73dc7123e98` | MIT, `llama.cpp/LICENSE` |
| `whisper.cpp/` | https://github.com/ggml-org/whisper.cpp | `v1.9.5` (d1be6fd) | `ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720` | MIT, `whisper.cpp/LICENSE` |

The archive is the release tag's source archive (`/archive/refs/tags/<tag>.tar.gz`). Kept from it, file
contents unchanged:

- `llama.cpp/`: `CMakeLists.txt`, `cmake/`, `include/`, `src/`, `LICENSE`, `licenses/`, `tools/mtmd/`,
  `vendor/` (`CMakeLists.txt`, `hash`, `miniaudio`, `nlohmann`, `sheredom`, `stb`), and `ggml/` with the
  CPU, Metal, BLAS, Vulkan and CUDA backends only.
- `whisper.cpp/`: `CMakeLists.txt`, `cmake/`, `include/`, `src/`, `LICENSE`, `bindings/CMakeLists.txt`
  and `bindings/javascript/package-tmpl.json` (both read by its CMake configuration). Its own copy of
  ggml is left out. `src/` carries the `parakeet` library and the ANEForge encoder source too: its CMake
  files build them unconditionally, and neither is linked or reached by the bindings.

Moving to a newer release: replace both directories from the new release archives the same way, update
this table, and compare the outputs of every local engine before and after.
