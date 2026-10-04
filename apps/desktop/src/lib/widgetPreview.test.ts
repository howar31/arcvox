import { describe, expect, it } from "vitest";
import { DEMO, SCENES, lookOf, previewRequest, sceneFor, type Look } from "./widgetPreview";

const look = (over: Partial<Look> = {}): Look => ({ widget_size: "medium", widget_idle: "hidden", widget_fade: { enabled: true, after_secs: 2, transparency: 70 }, show_transcript: true, ...over });
const sample = { raw: "raw", refined: "refined", detail: "detail" };

describe("sceneFor", () => {
  it("shows a change of the resting widget in the idle scene", () => {
    expect(sceneFor(look(), look({ widget_idle: "ring" }))).toBe("idle");
    expect(sceneFor(look(), look({ widget_fade: { enabled: false, after_secs: 2, transparency: 70 } }))).toBe("idle");
    expect(sceneFor(look(), look({ widget_fade: { enabled: true, after_secs: 2, transparency: 40 } }))).toBe("idle");
  });
  it("shows the switch of the recognised text in the processing scene", () => {
    expect(sceneFor(look(), look({ show_transcript: false }))).toBe("processing");
  });
  it("keeps the scene for the size and when nothing changed", () => {
    expect(sceneFor(look(), look({ widget_size: "x_large" }))).toBeNull();
    expect(sceneFor(look(), look())).toBeNull();
  });
});

describe("lookOf", () => {
  it("takes a copy that later edits do not reach", () => {
    const from = look();
    const copy = lookOf(from);
    from.widget_fade.transparency = 10;
    expect(copy.widget_fade.transparency).toBe(70);
  });
});

describe("previewRequest", () => {
  it("sends the sample texts only when the recognised text is shown", () => {
    expect(previewRequest("done", look(), sample, true, false)).toMatchObject({ scene: "done", size: "medium", idle: "hidden", raw: "raw", refined: "refined", detail: "detail", play: true, faint: false });
    expect(previewRequest("processing", look({ show_transcript: false }), sample, false, true)).toMatchObject({ raw: null, refined: null, play: false, faint: true });
  });
});

describe("DEMO", () => {
  it("plays known scenes and ends at rest", () => {
    for (const [scene] of DEMO) expect(SCENES).toContain(scene);
    expect(DEMO[DEMO.length - 1]).toEqual(["idle", 0]);
    expect(DEMO.slice(0, -1).every(([, ms]) => ms > 0)).toBe(true);
  });
});
