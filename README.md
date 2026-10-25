# Arcvox

[![License](https://img.shields.io/github/license/howar31/arcvox?style=flat-square)](LICENSE)
[![Release](https://img.shields.io/github/v/release/howar31/arcvox?style=flat-square&include_prereleases)](https://github.com/howar31/arcvox/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20(Apple%20silicon)-blue?style=flat-square)](https://github.com/howar31/arcvox/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/howar31/arcvox/ci.yml?style=flat-square&label=CI)](https://github.com/howar31/arcvox/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/howar31/arcvox/total?style=flat-square)](https://github.com/howar31/arcvox/releases)
[![Sponsor](https://img.shields.io/badge/Sponsor-donate.howar31.com-b4532c?style=flat-square&logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI+PHBhdGggZmlsbD0iI2ZmZiIgZD0iTTIwLjg0IDQuNjFhNS41IDUuNSAwIDAgMC03Ljc4IDBMMTIgNS42N2wtMS4wNi0xLjA2YTUuNSA1LjUgMCAwIDAtNy43OCA3Ljc4bDEuMDYgMS4wNkwxMiAyMS4yM2w3Ljc4LTcuNzggMS4wNi0xLjA2YTUuNSA1LjUgMCAwIDAgMC03Ljc4eiIvPjwvc3ZnPg==)](https://donate.howar31.com/)

English | [正體中文](README.zh-Hant.md)

Voice dictation for macOS: hold a key, speak, and the text is put into the application in front. Speech recognition and rewriting run on the Mac with downloaded models, or on a service you point it at. Made for Traditional Chinese (Taiwan), English and Japanese.

## Features

- **Dictate anywhere**: hold a shortcut to speak, or press it twice for hands-free dictation; Esc cancels. Shortcuts can be modifier keys, combinations, keypad keys or mouse buttons. The text is pasted, typed, written through Accessibility, or only copied, set per application.
- **Local engines**: speech recognition (whisper.cpp and llama.cpp speech models such as Breeze-ASR-25 and Qwen3-ASR) and text rewriting (Gemma 3, Qwen3 and other GGUF models) run on the Mac with Metal. Models are downloaded from a catalogue or imported from your own files. On macOS 26, Apple's dictation and Apple Intelligence can be used as engines too.
- **Network engines**: any OpenAI-compatible service (Ollama, LM Studio, vLLM, a self-hosted server or a cloud API) can be added for recognition or rewriting.
- **Profiles**: writing styles from templates (casual, formal, verbatim, polished, blank), chosen per application or per website, each with its own engines, prompt, dictionary use, Chinese script conversion (OpenCC) and optional translation.
- **Dictionary**: terms and their misheard forms; corrections you make after a dictation are read back and turned into suggestions.
- **History and statistics**: every dictation with its recording and the text of each processing stage, searchable and filterable; a dictation can be run again with another profile. Statistics cards and a shareable image of the figures.
- **Backup**: settings, content (profiles, engines, dictionary, prompts) and records (history, statistics, recordings) export and import separately, and can be backed up to a folder automatically.
- **Tests**: a speed test, and a quality test that scores engines on your own corrected dictations.
- Floating widget and menu-bar icon that show the state, light and dark themes, English and Traditional Chinese interface.
- **Privacy**: no account and no telemetry. With local engines, audio and text stay on the Mac. The app connects to the network to download models and the model catalogue, to check for updates, and to reach the network engines you add.

## Install

**macOS 14+ on Apple silicon** (Apple's dictation and language model as engines require macOS 26): download the DMG from [Releases](https://github.com/howar31/arcvox/releases) and drag Arcvox to Applications. The first launch opens a short guide that downloads a speech model and a rewrite model and asks for the Accessibility and Microphone permissions.

Installed copies update themselves: when a new release is out, the About page and the menu-bar menu offer it, and installing it restarts the app.

A Windows version is planned.

## Build from source

Requires Rust (stable), Node.js 24, CMake and Xcode 26 (the macOS 26 SDK).

```
cd apps/desktop
npm ci
npx tauri build
```

`core/` holds the Rust core (`arcvox-core`) and the local inference crate (`arcvox-inference`); `apps/desktop/` holds the Tauri app.

## Support

Bug reports and feature requests go to
[GitHub Issues](https://github.com/howar31/arcvox/issues/new/choose).
In the app, Settings > About > Report an issue opens the form.

If Arcvox is useful to you, you can sponsor development:
[Sponsor](https://donate.howar31.com/) · [Ko-fi](https://ko-fi.com/howar31)

## License

[Apache-2.0](LICENSE) © 2026 Howar31. Settings > About lists the licenses of the inference runtimes and of every package a build ships.

"Arcvox" is a trademark of the project owner. Forks must use a different name and logo.
