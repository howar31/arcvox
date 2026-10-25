import { expect, test } from "vitest";
import { depth, direction, NAV, navPage, parseRoute, routeHash, routeSection, shouldIntercept } from "./route";

test("a hash can name a section of the page", () => {
  expect(parseRoute("#general@permissions")).toEqual({ page: "general", arg: null });
  expect(routeSection("#general@permissions")).toBe("permissions");
  expect(parseRoute("#flow/mail@engine")).toEqual({ page: "flow", arg: "mail" });
  expect(routeSection("#flow/mail@engine")).toBe("engine");
  expect(routeSection("#general")).toBeNull();
  expect(routeSection("#general@")).toBeNull();
  expect(routeHash("general", null, "permissions")).toBe("#general@permissions");
  // An argument's own at sign is escaped, so it is never taken for a section.
  expect(routeHash("flow", "a@b")).toBe("#flow/a%40b");
  expect(routeSection(routeHash("flow", "a@b"))).toBeNull();
  expect(parseRoute(routeHash("flow", "a@b", "engine"))).toEqual({ page: "flow", arg: "a@b" });
});

test("an empty hash opens the overview", () => {
  expect(parseRoute("")).toEqual({ page: "home", arg: null });
  expect(parseRoute("#")).toEqual({ page: "home", arg: null });
});

test("a page with an argument", () => {
  expect(parseRoute("#flow/mail")).toEqual({ page: "flow", arg: "mail" });
  expect(parseRoute("#engines/llm")).toEqual({ page: "engines", arg: "llm" });
  expect(parseRoute("#general")).toEqual({ page: "general", arg: null });
});

test("the earlier profile page resolves to the flow page and keeps its argument", () => {
  expect(parseRoute("#profiles/mail")).toEqual({ page: "flow", arg: "mail" });
  expect(parseRoute("#profiles")).toEqual({ page: "flow", arg: null });
});

test("the old provider and model pages resolve to engines", () => {
  expect(parseRoute("#models")).toEqual({ page: "engines", arg: null });
  expect(parseRoute("#providers")).toEqual({ page: "engines", arg: null });
});

test("the licences page sits under About and can name a package", () => {
  expect(parseRoute("#licenses")).toEqual({ page: "licenses", arg: null });
  expect(parseRoute("#licenses/tokio")).toEqual({ page: "licenses", arg: "tokio" });
  expect(parseRoute(routeHash("licenses", "beta@rust"))).toEqual({ page: "licenses", arg: "beta@rust" });
  expect(navPage("licenses")).toBe("about");
  expect(parseRoute("#whatsnew")).toEqual({ page: "whatsnew", arg: null });
  expect(navPage("whatsnew")).toBe("about");
});

test("the first-launch guide is a page without a sidebar entry", () => {
  expect(parseRoute("#welcome")).toEqual({ page: "welcome", arg: null });
  expect(navPage("welcome")).toBe("home");
  expect(NAV.flatMap((g) => g.pages)).not.toContain("welcome");
});

test("unknown pages resolve to the overview", () => {
  expect(parseRoute("#nope")).toEqual({ page: "home", arg: null });
  expect(parseRoute("#nope/x")).toEqual({ page: "home", arg: null });
});

test("an argument round-trips through the hash", () => {
  expect(routeHash("flow", "a b")).toBe("#flow/a%20b");
  expect(parseRoute(routeHash("flow", "a b"))).toEqual({ page: "flow", arg: "a b" });
  expect(routeHash("general")).toBe("#general");
  expect(parseRoute(routeHash("engines", null))).toEqual({ page: "engines", arg: null });
});

test("the navigation lists every page once", () => {
  const pages = NAV.flatMap((g) => g.pages);
  expect(new Set(pages).size).toBe(pages.length);
  expect(NAV.map((g) => g.pages)).toEqual([["home", "history"], ["flow", "engines", "dictionary"], ["general", "recording", "data", "storage", "backup"], ["about"]]);
  expect(NAV.map((g) => g.group)).toEqual([null, "nav.group.dictation", "nav.group.settings", null]);
});

test("the base prompt has no sidebar entry and counts as the profiles page there", () => {
  expect(NAV.flatMap((g) => g.pages)).not.toContain("prompt");
  expect(parseRoute("#prompt")).toEqual({ page: "prompt", arg: null });
  expect(navPage("prompt")).toBe("flow");
});

test("the speed and quality tests have no sidebar entry and count as the engines page there", () => {
  expect(NAV.flatMap((g) => g.pages)).not.toContain("speed");
  expect(NAV.flatMap((g) => g.pages)).not.toContain("quality");
  expect(parseRoute("#speed")).toEqual({ page: "speed", arg: null });
  expect(parseRoute("#quality")).toEqual({ page: "quality", arg: null });
  expect(navPage("speed")).toBe("engines");
  expect(navPage("quality")).toBe("engines");
  expect(navPage("flow")).toBe("flow");
});

test("a hash change from outside is held only when the page has unsaved changes and the route differs", () => {
  const detail = { page: "flow" as const, arg: "mail" };
  expect(shouldIntercept(false, detail, { page: "general", arg: null })).toBe(false);
  expect(shouldIntercept(true, detail, { page: "general", arg: null })).toBe(true);
  expect(shouldIntercept(true, detail, { page: "flow", arg: "code" })).toBe(true);
  expect(shouldIntercept(true, detail, { page: "flow", arg: null })).toBe(true);
  expect(shouldIntercept(true, detail, { page: "flow", arg: "mail" })).toBe(false);
});

test("the backup page is a settings entry of its own, after data", () => {
  const settings = NAV.find((g) => g.group === "nav.group.settings")!.pages;
  expect(settings).toEqual(["general", "recording", "data", "storage", "backup"]);
  expect(parseRoute("#backup")).toEqual({ page: "backup", arg: null });
  expect(navPage("backup")).toBe("backup");
});

test("a page under an entry slides in, the way back slides out, other moves do not slide", () => {
  const r = (hash: string) => parseRoute(hash);
  expect(depth(r("#about"))).toBe(0);
  expect(depth(r("#licenses"))).toBe(1);
  expect(depth(r("#licenses/tokio"))).toBe(2);
  expect(depth(r("#quality"))).toBe(1);
  expect(depth(r("#quality/compare"))).toBe(2);
  expect(direction(r("#about"), r("#whatsnew"))).toBe(1);
  expect(direction(r("#licenses/tokio"), r("#about"))).toBe(-1);
  expect(direction(r("#flow/default"), r("#prompt/rewrite"))).toBe(1);
  expect(direction(r("#engines/llm"), r("#speed"))).toBe(1);
  expect(direction(r("#quality/result"), r("#quality"))).toBe(-1);
  // Another sidebar entry, a profile switch, a tab, a sibling package.
  expect(direction(r("#licenses"), r("#engines"))).toBe(0);
  expect(direction(r("#flow/a"), r("#flow/b"))).toBe(0);
  expect(direction(r("#engines/asr"), r("#engines/llm"))).toBe(0);
  expect(direction(r("#licenses/a"), r("#licenses/b"))).toBe(0);
});
