import { useCallback, useEffect, useRef, useState } from "react";
import { Microphone, micError } from "../audio/engine";
import { socketUrl } from "./api";

export type RecorderState = "idle" | "starting" | "recording" | "reconnecting" | "stopping";

export interface Line {
  time: string;
  text: string;
}

export interface Recorder {
  state: RecorderState;
  /** The note being recorded into. */
  path: string | null;
  startedAt: number | null;
  level: number;
  interim: string;
  /** Lines finished during this recording (already saved in the note). */
  lines: Line[];
  error: string | null;
  start: (path: string) => Promise<void>;
  stop: () => void;
  clearError: () => void;
}

/** Live transcription into a note: microphone → /ws/transcribe → lines appended to the file. */
export function useRecorder(): Recorder {
  const [state, setState] = useState<RecorderState>("idle");
  const [path, setPath] = useState<string | null>(null);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [level, setLevel] = useState(0);
  const [interim, setInterim] = useState("");
  const [lines, setLines] = useState<Line[]>([]);
  const [error, setError] = useState<string | null>(null);
  const ws = useRef<WebSocket | null>(null);
  const mic = useRef<Microphone | null>(null);
  const wake = useRef<WakeLockSentinel | null>(null);

  const cleanup = useCallback(() => {
    void mic.current?.close();
    mic.current = null;
    ws.current = null;
    void wake.current?.release().catch(() => {});
    wake.current = null;
    setState("idle");
    setLevel(0);
    setInterim("");
  }, []);

  const start = useCallback(
    async (target: string) => {
      if (ws.current) return;
      setError(null);
      setLines([]);
      setInterim("");
      setPath(target);
      setState("starting");
      const socket = new WebSocket(socketUrl(`/ws/transcribe?path=${encodeURIComponent(target)}`));
      socket.binaryType = "arraybuffer";
      ws.current = socket;
      socket.onmessage = (e) => {
        if (typeof e.data !== "string") return;
        const msg = JSON.parse(e.data) as { type: string; text?: string; time?: string; state?: string; message?: string };
        switch (msg.type) {
          case "ready":
            setState("recording");
            setStartedAt(Date.now());
            break;
          case "status":
            if (msg.state === "reconnecting") setState("reconnecting");
            else if (msg.state === "listening") setState((s) => (s === "stopping" ? s : "recording"));
            break;
          case "interim":
            setInterim(msg.text ?? "");
            break;
          case "final":
            setLines((l) => [...l, { time: msg.time ?? "", text: msg.text ?? "" }]);
            setInterim("");
            break;
          case "error":
            setError(msg.message ?? "Something went wrong");
            break;
          case "stopped":
            socket.close();
            break;
        }
      };
      socket.onclose = () => {
        if (ws.current === socket) cleanup();
      };
      try {
        mic.current = await Microphone.open(
          { echoCancellation: false },
          (pcm) => {
            if (socket.readyState === WebSocket.OPEN) socket.send(pcm);
          },
          setLevel,
        );
        wake.current = await navigator.wakeLock?.request("screen").catch(() => null);
      } catch (e) {
        setError(micError(e));
        socket.close();
        cleanup();
      }
    },
    [cleanup],
  );

  const stop = useCallback(() => {
    const socket = ws.current;
    if (!socket) return;
    setState("stopping");
    void mic.current?.close();
    mic.current = null;
    setLevel(0);
    if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: "stop" }));
    else socket.close();
    // If the server never answers, don't hang.
    window.setTimeout(() => {
      if (ws.current === socket) {
        socket.close();
        cleanup();
      }
    }, 5000);
  }, [cleanup]);

  // Leaving the page mid-recording loses the last sentence; ask first.
  useEffect(() => {
    if (state === "idle") return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [state]);

  return { state, path, startedAt, level, interim, lines, error, start, stop, clearError: () => setError(null) };
}
