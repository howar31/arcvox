import { expect, test } from "vitest";
import { NAV, navPage, parseRoute, routeHash, shouldIntercept } from "./route";

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
  expect(NAV.map((g) => g.pages)).toEqual([["home", "history"], ["flow", "engines", "prompt", "dictionary"], ["general", "recording", "data", "advanced"], ["about"]]);
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
