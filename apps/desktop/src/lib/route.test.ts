import { expect, test } from "vitest";
import { NAV, parseRoute, routeHash, shouldIntercept } from "./route";

test("an empty hash opens the overview", () => {
  expect(parseRoute("")).toEqual({ page: "home", arg: null });
  expect(parseRoute("#")).toEqual({ page: "home", arg: null });
});

test("a page with an argument", () => {
  expect(parseRoute("#profiles/mail")).toEqual({ page: "profiles", arg: "mail" });
  expect(parseRoute("#engines/llm")).toEqual({ page: "engines", arg: "llm" });
  expect(parseRoute("#general")).toEqual({ page: "general", arg: null });
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
  expect(routeHash("profiles", "a b")).toBe("#profiles/a%20b");
  expect(parseRoute(routeHash("profiles", "a b"))).toEqual({ page: "profiles", arg: "a b" });
  expect(routeHash("general")).toBe("#general");
  expect(parseRoute(routeHash("engines", null))).toEqual({ page: "engines", arg: null });
});

test("the navigation lists every page once", () => {
  const pages = NAV.flatMap((g) => g.pages);
  expect(new Set(pages).size).toBe(pages.length);
  expect(pages).toEqual(["home", "profiles", "engines", "dictionary", "prompt", "history", "general", "speed", "about"]);
});

test("a hash change from outside is held only when the page has unsaved changes and the route differs", () => {
  const detail = { page: "profiles" as const, arg: "mail" };
  expect(shouldIntercept(false, detail, { page: "general", arg: null })).toBe(false);
  expect(shouldIntercept(true, detail, { page: "general", arg: null })).toBe(true);
  expect(shouldIntercept(true, detail, { page: "profiles", arg: "code" })).toBe(true);
  expect(shouldIntercept(true, detail, { page: "profiles", arg: null })).toBe(true);
  expect(shouldIntercept(true, detail, { page: "profiles", arg: "mail" })).toBe(false);
});
