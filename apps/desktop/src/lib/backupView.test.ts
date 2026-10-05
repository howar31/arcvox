import { beforeEach, describe, expect, it } from "vitest";
import { setLanguage } from "./i18n";
import { deviceLine, holdLeft, importLines, itemsSummary, KIND_ITEMS, noteLines, RUN_MIN_MS, statusView, writtenLine } from "./backupView";

const cats = (settings: boolean, content: boolean, history: boolean, stats: boolean, recordings: boolean) => ({ settings, content, history, stats, recordings });
const report = { devices: 1, added: 0, updated: 0, skipped: 0, cells: 0, recordings: 0, missing_recordings: 0, bad_lines: 0, expiring: 0 };
const none = { rows: 0, cells: 0, recordings: 0, recording_bytes: 0, settings: false, recreated: false };
const all = cats(true, true, true, true, true);
const device = { id: "d", name: "Mini", platform: "macos", rows: 532, first_month: "2026-06", last_month: "2026-09", recordings: 118, recording_bytes: 52_000_000, has_stats: true, has_settings: false, this: false };

describe("backupView", () => {
  beforeEach(() => setLanguage("zh-TW"));

  it("keeps settings and content apart", () => {
    expect(KIND_ITEMS.settings).toEqual(["recording", "insertion", "general", "appearance"]);
    expect(KIND_ITEMS.content).toEqual(["profiles", "providers", "dictionary", "base_prompt"]);
  });

  it("names what is backed up to the folder, kind by kind", () => {
    expect(itemsSummary(cats(true, true, true, true, false))).toBe("設定、內容、歷史紀錄、統計");
    expect(itemsSummary(cats(false, false, false, false, false))).toBe("沒有選擇項目");
    setLanguage("en");
    expect(itemsSummary(cats(false, true, true, false, true))).toBe("Content, History, Recordings");
    expect(itemsSummary(cats(false, false, false, false, false))).toBe("Nothing selected");
  });

  it("turns each note into a line", () => {
    const lines = noteLines([
      { kind: "needs_key", engine: "OpenAI" },
      { kind: "engine_replaced", profile: "郵件", role: "llm" },
      { kind: "engine_replaced", profile: "郵件", role: "asr" },
      { kind: "engine_unavailable", engine: "Apple Dictation" },
      { kind: "unknown_section", name: "gestures" },
      { kind: "apps_not_installed", names: ["a.b", "c.d"] },
    ]);
    expect(lines.map((l) => l.text)).toEqual([
      "引擎「OpenAI」需要重新輸入 API 金鑰",
      "情境「郵件」指向的文字改寫引擎不存在，改用預設情境的引擎",
      "情境「郵件」指向的語音辨識引擎不存在，改用預設情境的引擎",
      "引擎「Apple Dictation」無法在這台電腦使用",
      "檔案內的「gestures」是這個版本不認得的項目，已略過",
      "匯入的項目提到 2 個這台沒有安裝的應用程式：a.b、c.d",
    ]);
    expect(lines[0].go).toBe("engines");
    expect(lines[0].detail).toBe("金鑰不會寫進內容檔。");
    expect(lines[1].go).toBeUndefined();
    expect(lines[5].detail).toBe("安裝後會自動生效。");
    setLanguage("en");
    expect(noteLines([{ kind: "apps_not_installed", names: ["a.b", "c.d"] }])[0].text).toBe("The imported items name 2 applications that are not installed here: a.b, c.d");
  });

  it("describes a device of a backup", () => {
    expect(deviceLine(device)).toBe("macOS。歷史 532 筆，2026-06 至 2026-09，錄音 118 個（52 MB）");
    expect(deviceLine({ ...device, rows: 96, first_month: "2026-08", last_month: "2026-08", recordings: 0, recording_bytes: 0 })).toBe("macOS。歷史 96 筆，2026-08，沒有錄音");
    expect(deviceLine({ ...device, platform: "plan9", rows: 0, first_month: null, last_month: null, recordings: 0, recording_bytes: 0 })).toBe("plan9。歷史 0 筆，沒有錄音");
    setLanguage("en");
    expect(deviceLine(device)).toBe("macOS. 532 history rows, 2026-06 to 2026-09, 118 recordings (52 MB)");
  });

  it("shows the state of the backup folder", () => {
    const time = (ts: number) => `T${ts}`;
    expect(statusView({ state: "never" }, time, all)).toEqual({ tone: "idle", title: "尚未備份", detail: "還沒有備份到這個資料夾。", fix: null, run: true });
    expect(statusView({ state: "ok", at: 5, report: none }, time, all)).toEqual({ tone: "ok", title: "已備份", detail: "T5 · 沒有新的變動", fix: null, run: true });
    expect(statusView({ state: "running" }, time, all)).toEqual({ tone: "busy", title: "備份中…", detail: "", fix: null, run: true });
    expect(statusView({ state: "failed", at: 9, reason: "the folder is gone", last_ok: 5 }, time, all)).toEqual({ tone: "bad", title: "無法備份", detail: "the folder is gone。上次成功：T5", fix: null, run: true });
    expect(statusView({ state: "failed", at: 9, reason: "the folder is gone", last_ok: null }, time, all).detail).toBe("the folder is gone");
  });

  it("puts a folder that cannot be used before the last write", () => {
    const time = (ts: number) => `T${ts}`;
    const ok = { state: "ok", at: 5, report: none } as const;
    expect(statusView(ok, time, all, "missing")).toEqual({ tone: "bad", title: "找不到備份資料夾", detail: "資料夾可能已被刪除、移動，或所在的磁碟沒有連接。上次備份：T5", fix: "create", run: false });
    expect(statusView(ok, time, all, "not_writable")).toEqual({ tone: "bad", title: "無法寫入備份資料夾", detail: "這個資料夾目前不允許寫入。上次備份：T5", fix: null, run: true });
    expect(statusView(ok, time, all, "blocked")).toEqual({ tone: "bad", title: "無法寫入備份資料夾", detail: "系統的隱私權設定沒有允許存取這個資料夾。上次備份：T5", fix: "system", run: true });
    // The last write that worked is named, whatever the write after it said.
    expect(statusView({ state: "failed", at: 9, reason: "the folder does not exist", last_ok: 5 }, time, all, "missing").detail).toBe("資料夾可能已被刪除、移動，或所在的磁碟沒有連接。上次備份：T5");
    expect(statusView({ state: "failed", at: 9, reason: "x", last_ok: null }, time, all, "missing").detail).toBe("資料夾可能已被刪除、移動，或所在的磁碟沒有連接。");
    expect(statusView({ state: "never" }, time, all, "missing").detail).toBe("資料夾可能已被刪除、移動，或所在的磁碟沒有連接。");
    // A write that is running is shown as running.
    expect(statusView({ state: "running" }, time, all, "missing").title).toBe("備份中…");
    expect(statusView(ok, time, all, "usable").fix).toBeNull();
    setLanguage("en");
    expect(statusView(ok, time, all, "missing").detail).toBe("The folder may have been deleted or moved, or its disk is not connected. Last backup: T5");
  });

  it("names the folder as the reason of a failed write in the interface language", () => {
    const time = (ts: number) => `T${ts}`;
    // The folder can be used again, and the write that failed has not been made up for yet.
    expect(statusView({ state: "failed", at: 9, reason: "the folder does not exist", last_ok: 5, folder: "missing" }, time, all)).toEqual({ tone: "bad", title: "無法備份", detail: "上次備份時找不到資料夾。上次成功：T5", fix: null, run: true });
    expect(statusView({ state: "failed", at: 9, reason: "x", last_ok: null, folder: "not_writable" }, time, all).detail).toBe("上次備份時無法寫入資料夾");
    expect(statusView({ state: "failed", at: 9, reason: "x", last_ok: null, folder: "blocked" }, time, all).detail).toBe("上次備份時系統沒有允許存取資料夾");
    expect(statusView({ state: "failed", at: 9, reason: "disk full", last_ok: null, folder: null }, time, all).detail).toBe("disk full");
  });

  it("says when a write made the folder again", () => {
    const time = (ts: number) => `T${ts}`;
    expect(statusView({ state: "ok", at: 5, report: { ...none, rows: 3, recreated: true } }, time, all).detail).toBe("T5 · 資料夾已重新建立 · 已寫入：歷史紀錄 3 筆");
  });

  it("says what a write to the folder changed", () => {
    expect(writtenLine(none, all)).toBe("沒有新的變動");
    expect(writtenLine({ ...none, rows: 1204, cells: 2, recordings: 1 }, all)).toBe("已寫入：歷史紀錄 1,204 筆、統計 2 個時段、錄音 1 個");
    expect(writtenLine({ ...none, settings: true }, all)).toBe("已寫入：設定、內容");
    expect(writtenLine({ ...none, rows: 3, settings: true }, cats(false, true, true, true, false))).toBe("已寫入：歷史紀錄 3 筆、內容");
    setLanguage("en");
    expect(writtenLine({ ...none, rows: 3, cells: 2, settings: true }, cats(true, false, true, true, false))).toBe("Written: 3 history rows, 2 hourly buckets of statistics, settings");
  });

  it("keeps a write on screen for a least time", () => {
    expect(holdLeft(null, 5000)).toBe(0);
    expect(holdLeft(5000, 5000)).toBe(RUN_MIN_MS);
    expect(holdLeft(5000, 5200)).toBe(RUN_MIN_MS - 200);
    expect(holdLeft(5000, 9000)).toBe(0);
    // A clock that went back does not make the wait longer.
    expect(holdLeft(5000, 1000)).toBe(RUN_MIN_MS);
  });

  it("reports what an import did", () => {
    expect(importLines({ ...report, added: 616, skipped: 12, cells: 40 }, 0)).toEqual([
      { tone: "ok", text: "歷史紀錄：加入 616 筆，更新 0 筆，略過 12 筆已存在的" },
      { tone: "ok", text: "統計：加入或更新 40 個時段" },
    ]);
    const all = importLines({ ...report, added: 3, recordings: 2, missing_recordings: 1, bad_lines: 4, expiring: 2 }, 90);
    expect(all.map((l) => l.tone)).toEqual(["ok", "ok", "ok", "warn", "warn", "warn"]);
    expect(all[5]).toEqual({ tone: "warn", text: "歷史紀錄保留天數為 90 天", detail: "加入的紀錄中有 2 筆超過這個天數，會在下一次每日清除時移除。統計不受影響。" });
    expect(importLines({ ...report, added: 1, expiring: 1 }, 0).length).toBe(2);
  });
});
