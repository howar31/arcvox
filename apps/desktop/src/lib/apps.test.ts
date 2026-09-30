import { beforeEach, expect, test, vi } from "vitest";

const appNames = vi.fn();
vi.mock("./api", () => ({ appNames: (ids: string[]) => appNames(ids) }));

const { appName, ensureAppNames } = await import("./apps.svelte");

beforeEach(() => appNames.mockReset());

test("resolves names once and falls back to the bundle id", async () => {
  appNames.mockResolvedValue({ "com.apple.mail": "Mail", "x.unknown": null });
  expect(appName("com.apple.mail")).toBe("com.apple.mail");
  await ensureAppNames(["com.apple.mail", "x.unknown", "com.apple.mail"]);
  expect(appNames).toHaveBeenCalledWith(["com.apple.mail", "x.unknown"]);
  expect(appName("com.apple.mail")).toBe("Mail");
  expect(appName("x.unknown")).toBe("x.unknown");
  await ensureAppNames(["com.apple.mail", "x.unknown"]);
  expect(appNames).toHaveBeenCalledTimes(1);
});

test("a failed lookup is retried later", async () => {
  appNames.mockRejectedValueOnce(new Error("down"));
  await ensureAppNames(["com.apple.Notes"]);
  expect(appName("com.apple.Notes")).toBe("com.apple.Notes");
  appNames.mockResolvedValueOnce({ "com.apple.Notes": "Notes" });
  await ensureAppNames(["com.apple.Notes"]);
  expect(appName("com.apple.Notes")).toBe("Notes");
});
