# Vendored sources

Official release sources from [ggml-org](https://github.com/ggml-org), trimmed to what the build needs.
`build.rs` builds ggml once with llama.cpp, then whisper.cpp against that ggml, so the binary holds a
single copy of every ggml symbol.

| Directory | Source | Release | Archive sha256 | Licence |
|---|---|---|---|---|
| `llama.cpp/` | https://github.com/ggml-org/llama.cpp | `b11074` (26394b4) | `26f42f948700ecc8b6c712ea78f7d6f77075fed479a85961fe8d928ad81ff5f9` | MIT, `llama.cpp/LICENSE` |
| `whisper.cpp/` | https://github.com/ggml-org/whisper.cpp | `v1.8.3` (2eeeba5) | `870ba21409cdf66697dc4db15ebdb13bc67037d76c7cc63756c81471d8f1731a` | MIT, `whisper.cpp/LICENSE` |

The archive is the release tag's source archive (`/archive/refs/tags/<tag>.tar.gz`). Kept from it, file
contents unchanged:

- `llama.cpp/`: `CMakeLists.txt`, `cmake/`, `include/`, `src/`, `LICENSE`, `licenses/`, `tools/mtmd/`,
  `vendor/` (`CMakeLists.txt`, `hash`, `miniaudio`, `nlohmann`, `sheredom`, `stb`), and `ggml/` with the
  CPU, Metal, BLAS, Vulkan and CUDA backends only.
- `whisper.cpp/`: `CMakeLists.txt`, `cmake/`, `include/`, `src/`, `LICENSE`, `bindings/CMakeLists.txt`
  and `bindings/javascript/package-tmpl.json` (both read by its CMake configuration). Its own copy of
  ggml is left out.

Moving to a newer release: replace both directories from the new release archives the same way, update
this table, and compare the outputs of every local engine before and after.
