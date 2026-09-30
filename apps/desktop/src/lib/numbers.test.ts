import { expect, it } from "vitest";
import { invalidFields } from "./numbers";

it("flags empty and out-of-range numbers", () => {
  expect(invalidFields([
    { key: "a", value: 5, min: 1 }, { key: "b", value: null }, { key: "c", value: 0, min: 1 },
    { key: "d", value: NaN }, { key: "e", value: 9, max: 8 },
  ])).toEqual(["b", "c", "d", "e"]);
});
