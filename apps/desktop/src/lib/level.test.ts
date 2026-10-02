import { describe, expect, it } from "vitest";
import { LevelHistory, eased, mapped } from "./level";

const rms = (db: number) => Math.pow(10, db / 20);

describe("mapped", () => {
  it("is 0 for silence and for anything under the floor", () => {
    expect(mapped(0)).toBe(0);
    expect(mapped(rms(-70))).toBe(0);
  });
  it("is 1 from the ceiling up", () => {
    expect(mapped(rms(-15))).toBeCloseTo(1);
    expect(mapped(1)).toBe(1);
  });
  it("is even in decibels between the two", () => {
    expect(mapped(rms(-35))).toBeCloseTo(0.5);
    expect(mapped(rms(-45))).toBeCloseTo(0.25);
  });
});

describe("eased", () => {
  it("rises faster than it falls", () => {
    const up = eased(0, 1), down = 1 - eased(1, 0);
    expect(up).toBeGreaterThan(down);
    expect(up).toBeLessThan(1);
    expect(down).toBeGreaterThan(0);
  });
  it("stays where it is when nothing changes", () => {
    expect(eased(0.4, 0.4)).toBe(0.4);
  });
});

describe("LevelHistory", () => {
  it("gives the newest value at age 0 and older ones after it", () => {
    const h = new LevelHistory(4);
    h.push(0.1); h.push(0.2); h.push(0.3);
    expect(h.at(0)).toBe(0.3);
    expect(h.at(1)).toBe(0.2);
    expect(h.at(2)).toBe(0.1);
  });
  it("is 0 before anything was pushed and past its length", () => {
    const h = new LevelHistory(2);
    expect(h.at(0)).toBe(0);
    h.push(0.5); h.push(0.6); h.push(0.7);
    expect(h.at(1)).toBe(0.6);
    expect(h.at(2)).toBe(0);
    expect(h.at(9)).toBe(0);
  });
  it("lies between two samples for an age between them", () => {
    const h = new LevelHistory(4);
    h.push(0.2); h.push(0.8);
    expect(h.at(0.5)).toBeCloseTo(0.5);
    expect(h.at(0.25)).toBeGreaterThan(0.5);
    expect(h.at(-1)).toBe(0.8);
  });
  it("is empty again after clear", () => {
    const h = new LevelHistory(4);
    h.push(0.9);
    h.clear();
    expect(h.at(0)).toBe(0);
  });
});
