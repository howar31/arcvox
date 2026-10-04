import { describe, expect, it } from "vitest";
import { resetOf, together } from "./reset";

const defaults = { hotkey: { hold_threshold_ms: 250, shortcuts: [["Fn"]] }, widget_display: null as string | null, show: true };
const fresh = () => structuredClone(defaults);
const say = (v: unknown) => `default ${JSON.stringify(v)}`;

describe("resetOf", () => {
  it("offers nothing while the value is the default", () => {
    expect(resetOf(fresh(), defaults, "hotkey.hold_threshold_ms", say)).toBeUndefined();
    expect(resetOf(fresh(), defaults, "hotkey.shortcuts", say)).toBeUndefined();
    expect(resetOf(fresh(), defaults, "widget_display", say)).toBeUndefined();
  });

  it("offers nothing before the defaults are known", () => {
    expect(resetOf(fresh(), null, "show", say)).toBeUndefined();
    expect(resetOf(null, defaults, "show", say)).toBeUndefined();
  });

  it("names the default and puts it back", () => {
    const now = fresh();
    now.hotkey.hold_threshold_ms = 400;
    const reset = resetOf(now, defaults, "hotkey.hold_threshold_ms", say)!;
    expect(reset.hint).toBe("default 250");
    reset.run();
    expect(now.hotkey.hold_threshold_ms).toBe(250);
    expect(resetOf(now, defaults, "hotkey.hold_threshold_ms", say)).toBeUndefined();
  });

  it("treats a field that was emptied as changed", () => {
    const now = fresh() as unknown as { hotkey: { hold_threshold_ms: number | null } };
    now.hotkey.hold_threshold_ms = null;
    expect(resetOf(now, defaults, "hotkey.hold_threshold_ms", say)).toBeDefined();
  });

  it("puts back a copy, so a later edit leaves the defaults alone", () => {
    const now = fresh();
    now.hotkey.shortcuts = [["F5"]];
    resetOf(now, defaults, "hotkey.shortcuts", say)!.run();
    now.hotkey.shortcuts[0].push("Shift");
    expect(defaults.hotkey.shortcuts).toEqual([["Fn"]]);
  });

  it("puts back a default that is held in a proxy", () => {
    const held = new Proxy(fresh(), { get: (o, k) => { const v = Reflect.get(o, k); return v && typeof v === "object" ? new Proxy(v, {}) : v; } });
    const now = fresh();
    now.hotkey.shortcuts = [["F5"]];
    resetOf(now, held, "hotkey.shortcuts", say)!.run();
    expect(now.hotkey.shortcuts).toEqual([["Fn"]]);
  });
});

describe("together", () => {
  it("is nothing when no value was changed", () => {
    expect(together([undefined, undefined])).toBeUndefined();
  });

  it("puts every changed value back", () => {
    const now = fresh();
    now.show = false;
    now.widget_display = "Monitor #2";
    const all = together([resetOf(now, defaults, "show", say), resetOf(now, defaults, "hotkey.shortcuts", say), resetOf(now, defaults, "widget_display", say)])!;
    all();
    expect(now).toEqual(defaults);
  });
});
