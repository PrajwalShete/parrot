import { useCallback, useEffect, useRef, useState } from "react";
import { Microphone, Speaker, micError } from "../audio/engine";
import { socketUrl } from "./api";

export type AssistantState = "idle" | "connecting" | "live" | "ending";

export type Item =
  | { id: number; role: "user" | "agent"; text: string; done: boolean; typed?: boolean }
  | { id: number; role: "tool"; key: string; name: string; summary: string; status: "running" | "done" | "error"; paths?: string[] };

export interface View {
  note: string | null;
  topic: string | null;
}

export interface Assistant {
  state: AssistantState;
  items: Item[];
  muted: boolean;
  /** The assistant is talking right now. */
  speaking: boolean;
  micLevel: number;
  outputLevel: () => number;
  error: string | null;
  start: () => Promise<void>;
  stop: () => void;
  send: (text: string) => void;
  setMuted: (m: boolean) => void;
  clear: () => void;
  clearError: () => void;
}

let nextId = 1;

/** Joins streamed transcript fragments, keeping spaces sensible. */
export function joinFragment(prev: string, next: string): string {
  if (!prev) return next.trimStart();
  if (!next) return prev;
  if (/\s$/.test(prev) || /^[\s.,!?;:।)\]]/.test(next)) return prev + next;
  return prev + " " + next;
}

/** Applies one streamed fragment: extends the open item for that role, or opens a new one. */
export function addFragment(items: Item[], role: "user" | "agent", text: string, typed = false): Item[] {
  const last = items[items.length - 1];
  if (last && last.role === role && !last.done) {
    return [...items.slice(0, -1), { ...last, text: joinFragment(last.text, text) }];
  }
  const closed = items.map((i) => (i.role !== "tool" && !i.done ? { ...i, done: true } : i));
  return [...closed, { id: nextId++, role, text: text.trim(), done: typed, typed }];
}

export function useAssistant(view: View, onChanged: (paths: string[]) => void): Assistant {
  const [state, setState] = useState<AssistantState>("idle");
  const [items, setItems] = useState<Item[]>([]);
  const [muted, setMutedState] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const [micLevel, setMicLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const ws = useRef<WebSocket | null>(null);
  const mic = useRef<Microphone | null>(null);
  const speaker = useRef<Speaker | null>(null);
  const viewRef = useRef(view);
  const changedRef = useRef(onChanged);
  changedRef.current = onChanged;

  const sendView = useCallback(() => {
    if (ws.current?.readyState === WebSocket.OPEN) ws.current.send(JSON.stringify({ type: "view", ...viewRef.current }));
  }, []);

  useEffect(() => {
    viewRef.current = view;
    sendView();
  }, [view, sendView]);

  const cleanup = useCallback(() => {
    void mic.current?.close();
    void speaker.current?.close();
    mic.current = null;
    speaker.current = null;
    ws.current = null;
    setState("idle");
    setSpeaking(false);
    setMicLevel(0);
    setItems((list) => list.map((i) => (i.role === "tool" ? (i.status === "running" ? { ...i, status: "error" as const } : i) : { ...i, done: true })));
  }, []);

  const start = useCallback(async () => {
    if (ws.current) return;
    setError(null);
    setState("connecting");
    let sp: Speaker;
    try {
      // Created on the click, so the browser lets it play.
      sp = await Speaker.open(setSpeaking);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setState("idle");
      return;
    }
    speaker.current = sp;
    const socket = new WebSocket(socketUrl("/ws/agent"));
    socket.binaryType = "arraybuffer";
    ws.current = socket;
    socket.onopen = () => sendView();
    socket.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        speaker.current?.push(e.data);
        return;
      }
      const msg = JSON.parse(e.data as string) as {
        type: string;
        text?: string;
        typed?: boolean;
        id?: string;
        name?: string;
        status?: "running" | "done" | "error";
        summary?: string;
        paths?: string[];
        message?: string;
        state?: string;
      };
      switch (msg.type) {
        case "ready":
          setState("live");
          break;
        case "user_text":
          setItems((l) => addFragment(l, "user", msg.text ?? "", msg.typed));
          break;
        case "agent_text":
          setItems((l) => addFragment(l, "agent", msg.text ?? ""));
          break;
        case "turn_complete":
          setItems((l) => l.map((i) => (i.role !== "tool" && !i.done ? { ...i, done: true } : i)));
          break;
        case "interrupted":
          speaker.current?.flush();
          setItems((l) => l.map((i) => (i.role === "agent" && !i.done ? { ...i, done: true } : i)));
          break;
        case "tool": {
          const key = msg.id ?? "";
          setItems((l) => {
            const existing = l.find((i) => i.role === "tool" && i.key === key);
            const summary = msg.summary ?? labelFor(msg.name ?? "");
            if (existing) {
              return l.map((i) => (i === existing ? { ...i, status: msg.status ?? "done", summary, paths: msg.paths } : i));
            }
            const closed = l.map((i) => (i.role !== "tool" && !i.done ? { ...i, done: true } : i));
            return [...closed, { id: nextId++, role: "tool", key, name: msg.name ?? "", summary, status: msg.status ?? "running" }];
          });
          break;
        }
        case "changed":
          changedRef.current(msg.paths ?? []);
          break;
        case "status":
          if (msg.state === "live") setState("live");
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
        { echoCancellation: true },
        (pcm) => {
          if (socket.readyState === WebSocket.OPEN) socket.send(pcm);
        },
        setMicLevel,
      );
    } catch (e) {
      // Keep the session: typing still works without a microphone.
      setError(`${micError(e)} You can still type to the assistant.`);
    }
  }, [cleanup, sendView]);

  const stop = useCallback(() => {
    const socket = ws.current;
    if (!socket) return;
    setState("ending");
    speaker.current?.flush();
    if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: "stop" }));
    else socket.close();
    window.setTimeout(() => {
      if (ws.current === socket) {
        socket.close();
        cleanup();
      }
    }, 3000);
  }, [cleanup]);

  const send = useCallback((text: string) => {
    const t = text.trim();
    if (t && ws.current?.readyState === WebSocket.OPEN) ws.current.send(JSON.stringify({ type: "text", text: t }));
  }, []);

  const setMuted = useCallback((m: boolean) => {
    setMutedState(m);
    mic.current?.setMuted(m);
  }, []);

  useEffect(() => () => ws.current?.close(), []);

  return {
    state,
    items,
    muted,
    speaking,
    micLevel,
    outputLevel: () => speaker.current?.level() ?? 0,
    error,
    start,
    stop,
    send,
    setMuted,
    clear: () => setItems([]),
    clearError: () => setError(null),
  };
}

function labelFor(name: string): string {
  return (
    {
      get_current_view: "Checking the open note",
      list_notes: "Looking through your notes",
      read_note: "Reading",
      search_notes: "Searching",
      create_note: "Creating a note",
      edit_note: "Editing",
      append_to_note: "Adding to a note",
      write_note: "Writing",
      replace_in_notes: "Replacing across notes",
      create_topic: "Creating a topic",
      rename_item: "Renaming",
      move_item: "Moving",
      delete_item: "Moving to trash",
      run_command: "Running a command",
    }[name] ?? name
  );
}
