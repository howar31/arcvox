export interface WavInfo { sampleRate: number; channels: number; samples: number; durationMs: number; dataOffset: number }

function tag(view: DataView, at: number): string {
  return String.fromCharCode(view.getUint8(at), view.getUint8(at + 1), view.getUint8(at + 2), view.getUint8(at + 3));
}

/** Reads the header of a 16-bit PCM WAV file; null when the bytes are not one. */
export function parseWav(buffer: ArrayBuffer): WavInfo | null {
  if (buffer.byteLength < 12) return null;
  const view = new DataView(buffer);
  if (tag(view, 0) !== "RIFF" || tag(view, 8) !== "WAVE") return null;
  let sampleRate = 0, channels = 0, bits = 0;
  let at = 12;
  while (at + 8 <= buffer.byteLength) {
    const id = tag(view, at);
    const size = view.getUint32(at + 4, true);
    if (id === "fmt " && at + 24 <= buffer.byteLength) {
      channels = view.getUint16(at + 10, true);
      sampleRate = view.getUint32(at + 12, true);
      bits = view.getUint16(at + 22, true);
    } else if (id === "data") {
      if (bits !== 16 || channels < 1 || sampleRate < 1) return null;
      const bytes = Math.min(size, buffer.byteLength - (at + 8));
      const samples = Math.floor(bytes / 2 / channels);
      return { sampleRate, channels, samples, durationMs: Math.round((samples * 1000) / sampleRate), dataOffset: at + 8 };
    }
    // Chunks are padded to an even length.
    at += 8 + size + (size % 2);
  }
  return null;
}

/** `count` peak values from 0 to 1 across the whole clip, first channel. */
export function peaks(buffer: ArrayBuffer, info: WavInfo, count: number): number[] {
  const view = new DataView(buffer);
  const out = new Array<number>(count).fill(0);
  if (info.samples === 0) return out;
  const stride = info.channels * 2;
  for (let i = 0; i < count; i++) {
    const from = Math.floor((i * info.samples) / count);
    const to = Math.max(from + 1, Math.floor(((i + 1) * info.samples) / count));
    let max = 0;
    for (let s = from; s < to && s < info.samples; s++) max = Math.max(max, Math.abs(view.getInt16(info.dataOffset + s * stride, true)));
    out[i] = max / 32768;
  }
  return out;
}
