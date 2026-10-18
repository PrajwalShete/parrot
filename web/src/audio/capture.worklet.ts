// Runs on the audio thread: mono-mixes the microphone, converts to 16 kHz PCM16 and posts
// 100 ms chunks (transferring the buffer), plus a level about 20 times a second.

import { Downsampler, rms } from "./resample";

const CHUNK = 1600; // 100 ms at 16 kHz

class CaptureProcessor extends AudioWorkletProcessor {
  private readonly down = new Downsampler(sampleRate, 16000);
  private buf = new Int16Array(CHUNK);
  private fill = 0;
  private levelAcc = 0;
  private levelFrames = 0;

  process(inputs: Float32Array[][]): boolean {
    const channels = inputs[0];
    if (!channels || channels.length === 0 || !channels[0]) return true;
    const frames = channels[0].length;
    let mono: Float32Array;
    if (channels.length === 1) {
      mono = channels[0];
    } else {
      mono = new Float32Array(frames);
      for (const ch of channels) for (let i = 0; i < frames; i++) mono[i] = (mono[i] ?? 0) + (ch[i] ?? 0) / channels.length;
    }

    const level = rms(mono);
    this.levelAcc = Math.max(this.levelAcc, level);
    this.levelFrames += frames;
    if (this.levelFrames >= sampleRate / 20) {
      this.port.postMessage({ type: "level", value: this.levelAcc });
      this.levelAcc = 0;
      this.levelFrames = 0;
    }

    const pcm = this.down.process(mono);
    for (let i = 0; i < pcm.length; i++) {
      this.buf[this.fill++] = pcm[i] ?? 0;
      if (this.fill === CHUNK) {
        this.port.postMessage({ type: "chunk", buffer: this.buf.buffer }, [this.buf.buffer]);
        this.buf = new Int16Array(CHUNK);
        this.fill = 0;
      }
    }
    return true;
  }
}

registerProcessor("parrot-capture", CaptureProcessor);
