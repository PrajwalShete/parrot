// The microphone (→ 16 kHz PCM16 chunks) and the speaker (← 24 kHz PCM16 chunks).

import captureUrl from "./capture.worklet.ts?worker&url";
import playerUrl from "./player.worklet.ts?worker&url";
import { meter } from "./resample";

export interface MicOptions {
  /** On for the assistant (so it doesn't hear itself); off for recording a room. */
  echoCancellation: boolean;
}

/** A person-readable reason the microphone couldn't start. */
export function micError(e: unknown): string {
  const name = e instanceof DOMException ? e.name : "";
  if (name === "NotAllowedError" || name === "SecurityError")
    return "Microphone access is blocked. Allow it in the address bar (or System Settings → Privacy → Microphone), then try again.";
  if (name === "NotFoundError" || name === "OverconstrainedError") return "No microphone found. Plug one in and try again.";
  if (name === "NotReadableError") return "The microphone is busy in another app. Close it there and try again.";
  return e instanceof Error ? e.message : String(e);
}

export class Microphone {
  private muted = false;

  private constructor(
    private readonly ctx: AudioContext,
    private readonly stream: MediaStream,
    private readonly node: AudioWorkletNode,
    readonly label: string,
  ) {}

  static async open(opts: MicOptions, onChunk: (pcm: ArrayBuffer) => void, onLevel: (v: number) => void): Promise<Microphone> {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: opts.echoCancellation,
        noiseSuppression: true,
        autoGainControl: true,
      },
    });
    const ctx = new AudioContext({ latencyHint: "interactive" });
    try {
      await ctx.audioWorklet.addModule(captureUrl);
      const source = ctx.createMediaStreamSource(stream);
      const node = new AudioWorkletNode(ctx, "parrot-capture", { numberOfInputs: 1, numberOfOutputs: 0 });
      source.connect(node);
      const mic = new Microphone(ctx, stream, node, stream.getAudioTracks()[0]?.label ?? "Microphone");
      node.port.onmessage = (e: MessageEvent) => {
        const msg = e.data as { type: string; buffer?: ArrayBuffer; value?: number };
        if (msg.type === "chunk" && msg.buffer && !mic.muted) onChunk(msg.buffer);
        else if (msg.type === "level") onLevel(mic.muted ? 0 : meter(msg.value ?? 0));
      };
      if (ctx.state === "suspended") await ctx.resume();
      return mic;
    } catch (e) {
      stream.getTracks().forEach((t) => t.stop());
      await ctx.close();
      throw e;
    }
  }

  setMuted(muted: boolean) {
    this.muted = muted;
  }

  async close() {
    this.node.port.onmessage = null;
    this.node.disconnect();
    this.stream.getTracks().forEach((t) => t.stop());
    await this.ctx.close().catch(() => {});
  }
}

export class Speaker {
  private readonly data: Uint8Array<ArrayBuffer>;

  private constructor(
    private readonly ctx: AudioContext,
    private readonly node: AudioWorkletNode,
    private readonly analyser: AnalyserNode,
  ) {
    this.data = new Uint8Array(analyser.fftSize);
  }

  static async open(onPlaying: (playing: boolean) => void): Promise<Speaker> {
    const ctx = new AudioContext({ sampleRate: 24000, latencyHint: "interactive" });
    await ctx.audioWorklet.addModule(playerUrl);
    const node = new AudioWorkletNode(ctx, "parrot-player", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [1] });
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 512;
    node.connect(analyser);
    analyser.connect(ctx.destination);
    node.port.onmessage = (e: MessageEvent) => onPlaying((e.data as { type: string }).type === "start");
    if (ctx.state === "suspended") await ctx.resume();
    return new Speaker(ctx, node, analyser);
  }

  push(pcm: ArrayBuffer) {
    this.node.port.postMessage({ type: "chunk", buffer: pcm }, [pcm]);
  }

  flush() {
    this.node.port.postMessage({ type: "flush" });
  }

  /** Output level, 0..1, for the orb. */
  level(): number {
    this.analyser.getByteTimeDomainData(this.data);
    let sum = 0;
    for (const v of this.data) {
      const x = (v - 128) / 128;
      sum += x * x;
    }
    return meter(Math.sqrt(sum / this.data.length));
  }

  async close() {
    this.node.port.onmessage = null;
    this.node.disconnect();
    await this.ctx.close().catch(() => {});
  }
}
