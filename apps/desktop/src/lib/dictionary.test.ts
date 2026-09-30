import { expect, it } from "vitest";
import { trimEntries } from "./dictionary";

it("trims terms and aliases so a trailing newline keeps a term a term", () => {
  expect(trimEntries([
    { term: "Arcvox\n", aliases: [" k ", " "], asr_hint: true },
    { term: "\nBest regards,\nThe team\n", aliases: ["sig"], asr_hint: false },
  ])).toEqual([
    { term: "Arcvox", aliases: ["k"], asr_hint: true },
    { term: "Best regards,\nThe team", aliases: ["sig"], asr_hint: false },
  ]);
});
