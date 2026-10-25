import { describe, expect, it } from "vitest";
import { releaseNotes } from "./releaseNotes";

const body = `A release with two changes.

## What's new

<!-- arcvox:notes lang=en -->
- **Faster start**: the window opens sooner.
- **Share card**: figures as an image.
<!-- /arcvox:notes -->

<details><summary>正體中文</summary>

<!-- arcvox:notes lang=zh -->
- **啟動更快**：視窗更早開啟。
- **分享圖**：把數字做成圖片。
<!-- /arcvox:notes -->

</details>

**Full Changelog**: https://example.com/compare`;

describe("releaseNotes", () => {
  it("takes the bullets of the interface language", () => {
    expect(releaseNotes(body, "zh-TW")).toEqual([
      { lead: "啟動更快", detail: "視窗更早開啟。" },
      { lead: "分享圖", detail: "把數字做成圖片。" },
    ]);
    expect(releaseNotes(body, "en")[0]).toEqual({ lead: "Faster start", detail: "the window opens sooner." });
  });

  it("falls back to English, and gives nothing without a block", () => {
    expect(releaseNotes(body, "ja")).toHaveLength(2);
    expect(releaseNotes(body, "ja")[1].lead).toBe("Share card");
    expect(releaseNotes("Just a plain description.", "en")).toEqual([]);
    expect(releaseNotes(null, "en")).toEqual([]);
  });

  it("keeps a bullet without a bold lead", () => {
    const plain = "<!-- arcvox:notes lang=en -->\n- A fix for the widget.\n<!-- /arcvox:notes -->";
    expect(releaseNotes(plain, "en")).toEqual([{ lead: "", detail: "A fix for the widget." }]);
  });
});
