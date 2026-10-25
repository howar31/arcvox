import { expect, test } from "vitest";
import { sections, titleVersions } from "./whatsNew";

const notes = (lead: string) => `<!-- arcvox:notes lang=en -->\n- **${lead}**: detail\n<!-- /arcvox:notes -->`;

test("a release without notes drops out and the order stays", () => {
  const list = sections([{ version: "0.3.0", notes: notes("C") }, { version: "0.2.0", notes: "plain text" }, { version: "0.1.1", notes: notes("A") }], "en");
  expect(list.map((s) => s.version)).toEqual(["0.3.0", "0.1.1"]);
  expect(list[0].notes).toEqual([{ lead: "C", detail: "Detail" }]);
});

test("the title names one version, or the oldest and the newest", () => {
  expect(titleVersions([])).toBeNull();
  expect(titleVersions(sections([{ version: "0.2.0", notes: notes("A") }], "en"))).toEqual({ version: "0.2.0" });
  expect(titleVersions(sections([{ version: "0.3.0", notes: notes("B") }, { version: "0.2.0", notes: notes("A") }], "en"))).toEqual({ from: "0.2.0", to: "0.3.0" });
});
