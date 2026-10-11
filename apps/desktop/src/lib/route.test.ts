import { expect, test } from "vitest";
import { NAV, navPage, parseRoute, routeHash, routeSection, shouldIntercept } from "./route";

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
  expect(NAV.map((g) => g.pages)).toEqual([["home", "history"], ["flow", "engines", "prompt", "dictionary"], ["general", "recording", "data", "backup"], ["about"]]);
  expect(NAV.map((g) => g.group)).toEqual([null, "nav.group.dictation", "nav.group.settings", null]);
});

test("the speed test has no sidebar entry and counts as the engines page there", () => {
  expect(NAV.flatMap((g) => g.pages)).not.toContain("speed");
  expect(parseRoute("#speed")).toEqual({ page: "speed", arg: null });
  expect(navPage("speed")).toBe("engines");
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
  expect(settings).toEqual(["general", "recording", "data", "backup"]);
  expect(parseRoute("#backup")).toEqual({ page: "backup", arg: null });
  expect(navPage("backup")).toBe("backup");
});
