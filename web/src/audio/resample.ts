// Streaming conversion from the microphone's rate (usually 48 kHz) to 16 kHz PCM16 for Gemini.
// A one-pole low-pass keeps aliasing down, then linear interpolation picks the samples.
// Pure and stateful across calls, so it runs inside the capture worklet and in tests.

export class Downsampler {
  private readonly step: number;
  private readonly alpha: number;
  private pos = 0;
  private prev = 0;
  private lp = 0;

  constructor(from: number, readonly to = 16000) {
    this.step = from / to;
    if (this.step > 1) {
      const rc = 1 / (2 * Math.PI * Math.min(7000, to * 0.45));
      const dt = 1 / from;
      this.alpha = dt / (rc + dt);
    } else {
      this.alpha = 1;
    }
  }

  /** Converts one block. Returns 16-bit samples at the target rate. */
  process(input: Float32Array): Int16Array {
    const n = input.length;
    if (n === 0) return new Int16Array(0);
    const filtered = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      this.lp += this.alpha * ((input[i] ?? 0) - this.lp);
      filtered[i] = this.lp;
    }
    // Index 0 is the previous block's last sample; index k is filtered[k - 1].
    const at = (k: number) => (k === 0 ? this.prev : (filtered[k - 1] ?? 0));
    const out: number[] = [];
    while (this.pos < n) {
      const i = Math.floor(this.pos);
      const frac = this.pos - i;
      const s = at(i) + (at(i + 1) - at(i)) * frac;
      const clamped = Math.max(-1, Math.min(1, s));
      out.push(Math.round(clamped * 32767));
      this.pos += this.step;
    }
    this.pos -= n;
    this.prev = filtered[n - 1] ?? 0;
    return Int16Array.from(out);
  }
}

/** Root-mean-square level of a block, 0..1. */
export function rms(block: Float32Array): number {
  let sum = 0;
  for (let i = 0; i < block.length; i++) {
    const v = block[i] ?? 0;
    sum += v * v;
  }
  return block.length ? Math.sqrt(sum / block.length) : 0;
}

/** Maps RMS to a 0..1 meter value that moves for quiet rooms too. */
export function meter(level: number): number {
  if (level <= 0) return 0;
  const db = 20 * Math.log10(level);
  return Math.max(0, Math.min(1, (db + 60) / 50));
}

export function int16ToFloat32(pcm: Int16Array): Float32Array {
  const out = new Float32Array(pcm.length);
  for (let i = 0; i < pcm.length; i++) out[i] = (pcm[i] ?? 0) / 32768;
  return out;
}
