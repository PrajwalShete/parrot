// Plays the assistant's voice: PCM16 chunks arrive over the port and are queued; "flush" drops
// what's queued (the person interrupted). Reports when it starts and stops making sound.

class PlayerProcessor extends AudioWorkletProcessor {
  private queue: Float32Array[] = [];
  private offset = 0;
  private playing = false;

  constructor() {
    super();
    this.port.onmessage = (e: MessageEvent) => {
      const msg = e.data as { type: string; buffer?: ArrayBuffer };
      if (msg.type === "chunk" && msg.buffer) {
        const pcm = new Int16Array(msg.buffer);
        const f = new Float32Array(pcm.length);
        for (let i = 0; i < pcm.length; i++) f[i] = (pcm[i] ?? 0) / 32768;
        this.queue.push(f);
      } else if (msg.type === "flush") {
        this.queue = [];
        this.offset = 0;
      }
    };
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const out = outputs[0]?.[0];
    if (!out) return true;
    let i = 0;
    while (i < out.length && this.queue.length > 0) {
      const head = this.queue[0]!;
      const n = Math.min(out.length - i, head.length - this.offset);
      out.set(head.subarray(this.offset, this.offset + n), i);
      i += n;
      this.offset += n;
      if (this.offset >= head.length) {
        this.queue.shift();
        this.offset = 0;
      }
    }
    out.fill(0, i);
    const now = i > 0;
    if (now !== this.playing) {
      this.playing = now;
      this.port.postMessage({ type: now ? "start" : "end" });
    }
    return true;
  }
}

registerProcessor("parrot-player", PlayerProcessor);
