import { describe, expect, it } from "vitest";
import { parseWav, peaks } from "./wav";

/** A 16-bit mono WAV file with the given samples. */
function wav(samples: number[], sampleRate = 16000, extraChunk = false): ArrayBuffer {
  const extra = extraChunk ? 12 : 0;
  const buf = new ArrayBuffer(44 + extra + samples.length * 2);
  const v = new DataView(buf);
  const tag = (at: number, s: string) => [...s].forEach((c, i) => v.setUint8(at + i, c.charCodeAt(0)));
  tag(0, "RIFF"); v.setUint32(4, buf.byteLength - 8, true); tag(8, "WAVE");
  tag(12, "fmt "); v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
  v.setUint32(24, sampleRate, true); v.setUint32(28, sampleRate * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  let at = 36;
  if (extraChunk) { tag(at, "LIST"); v.setUint32(at + 4, 4, true); at += 12; }
  tag(at, "data"); v.setUint32(at + 4, samples.length * 2, true);
  samples.forEach((s, i) => v.setInt16(at + 8 + i * 2, s, true));
  return buf;
}

describe("wav", () => {
  it("reads the header and the length", () => {
    const buf = wav([...Array(8000).fill(0), ...Array(8000).fill(16384)]);
    const info = parseWav(buf)!;
    expect(info).toEqual({ sampleRate: 16000, channels: 1, samples: 16000, durationMs: 1000, dataOffset: 44 });
    expect(peaks(buf, info, 4).map((p) => Math.round(p * 100) / 100)).toEqual([0, 0, 0.5, 0.5]);
  });

  it("finds the data after another chunk", () => {
    const info = parseWav(wav([1, 2, 3, 4], 16000, true))!;
    expect(info.dataOffset).toBe(56);
    expect(info.samples).toBe(4);
  });

  it("rejects what is not a 16-bit WAV file", () => {
    expect(parseWav(new ArrayBuffer(10))).toBeNull();
    const notRiff = wav([1, 2]);
    new DataView(notRiff).setUint8(0, 0x58);
    expect(parseWav(notRiff)).toBeNull();
  });

  it("gives flat peaks for an empty clip", () => {
    const buf = wav([]);
    expect(peaks(buf, parseWav(buf)!, 3)).toEqual([0, 0, 0]);
  });
});
