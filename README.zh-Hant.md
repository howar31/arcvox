# Arcvox

[![License](https://img.shields.io/github/license/howar31/arcvox?style=flat-square)](LICENSE)
[![Release](https://img.shields.io/github/v/release/howar31/arcvox?style=flat-square&include_prereleases)](https://github.com/howar31/arcvox/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20(Apple%20silicon)-blue?style=flat-square)](https://github.com/howar31/arcvox/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/howar31/arcvox/ci.yml?style=flat-square&label=CI)](https://github.com/howar31/arcvox/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/howar31/arcvox/total?style=flat-square)](https://github.com/howar31/arcvox/releases)
[![Sponsor](https://img.shields.io/badge/%E8%B4%8A%E5%8A%A9-donate.howar31.com-b4532c?style=flat-square&logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI+PHBhdGggZmlsbD0iI2ZmZiIgZD0iTTIwLjg0IDQuNjFhNS41IDUuNSAwIDAgMC03Ljc4IDBMMTIgNS42N2wtMS4wNi0xLjA2YTUuNSA1LjUgMCAwIDAtNy43OCA3Ljc4bDEuMDYgMS4wNkwxMiAyMS4yM2w3Ljc4LTcuNzggMS4wNi0xLjA2YTUuNSA1LjUgMCAwIDAgMC03Ljc4eiIvPjwvc3ZnPg==)](https://donate.howar31.com/)

[English](README.md) | 正體中文

macOS 的語音聽寫工具：按住按鍵說話，文字就填入目前使用中的應用程式。語音辨識與文字改寫可以用下載到 Mac 上的模型執行，也可以交給你指定的服務。為正體中文（台灣）、英文與日文設計。

## 特色

- **到處都能聽寫**：按住快捷鍵說話，或按兩下進入免持聽寫；按 Esc 取消。快捷鍵可以是修飾鍵、組合鍵、數字鍵盤按鍵或滑鼠按鍵。文字可以用貼上、模擬打字、透過輔助使用寫入，或只複製到剪貼簿，依應用程式分別設定。
- **本機引擎**：語音辨識（whisper.cpp 與 llama.cpp 的語音模型，例如 Breeze-ASR-25、Qwen3-ASR）和文字改寫（Gemma 3、Qwen3 與其他 GGUF 模型）在 Mac 上以 Metal 執行。模型可以從目錄下載，也可以匯入自己的檔案。macOS 26 上也能把 Apple 的聽寫與 Apple Intelligence 當作引擎。
- **網路引擎**：任何相容 OpenAI 的服務（Ollama、LM Studio、vLLM、自架伺服器或雲端 API）都能加入，用於辨識或改寫。
- **情境**：從範本（休閒、正式、逐字、修飾、空白）建立寫作風格，依應用程式或網站套用，各自設定引擎、提示詞、字典、中文字形轉換（OpenCC）與選用的翻譯。
- **字典**：詞彙與它被聽錯的寫法；聽寫後你修改的內容會被讀回，整理成建議。
- **歷史紀錄與統計**：每次聽寫都保留錄音與每個處理階段的文字，可搜尋、篩選，也能換一個情境重新處理。統計卡片，以及可分享的數據圖片。
- **備份**：設定、內容（情境、引擎、字典、提示詞）與紀錄（歷史、統計、錄音）分開匯出與匯入，也能自動備份到資料夾。
- **測試**：速度測試，以及用你自己修正過的聽寫為引擎評分的品質測試。
- 顯示狀態的浮動 widget 與選單列圖示、淺色與深色主題、英文與正體中文介面。
- **隱私**：不需帳號，不蒐集使用資料。使用本機引擎時，聲音與文字不會離開 Mac。app 只在下載模型與模型目錄、檢查更新、連線到你加入的網路引擎時使用網路。

## 安裝

**macOS 14 以上、Apple silicon**（把 Apple 的聽寫與語言模型當作引擎需要 macOS 26）：從 [Releases](https://github.com/howar31/arcvox/releases) 下載 DMG，把 Arcvox 拖進「應用程式」。第一次開啟時有簡短的引導，會下載一個語音辨識模型與一個改寫模型，並請你允許輔助使用與麥克風權限。

安裝後會自動更新：有新版時，關於頁與選單列選單會提示，安裝後 app 會重新啟動。關於頁也能開啟每個版本的變更內容。

Windows 版規劃中。

## 從原始碼建置

需要 Rust（stable）、Node.js 24、CMake 與 Xcode 26（macOS 26 SDK）。

```
cd apps/desktop
npm ci
npx tauri build
```

`core/` 是 Rust 核心（`arcvox-core`）與本機推論模組（`arcvox-inference`）；`apps/desktop/` 是 Tauri app。

## 支援

問題回報與功能建議請到
[GitHub Issues](https://github.com/howar31/arcvox/issues/new/choose)。
在 app 中，設定 > 關於 > 回報問題會開啟回報表單。

如果 Arcvox 對你有幫助，歡迎贊助開發：
[贊助](https://donate.howar31.com/) · [Ko-fi](https://ko-fi.com/howar31)

## 授權

[Apache-2.0](LICENSE) © 2026 Howar31。設定 > 關於列出推論執行環境與每個隨版本附帶的套件授權。

「Arcvox」是專案擁有者的商標。fork 必須使用不同的名稱與標誌。
