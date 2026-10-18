import { ArrowUp, Check, Mic, MicOff, PanelRightClose, PhoneOff, RotateCcw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Assistant, Item } from "../lib/useAssistant";
import { Button, IconButton, Spinner } from "./primitives";

const SUGGESTIONS = [
  "Summarize this meeting with action items",
  "What did we decide about sales this week?",
  "Find every meeting that mentions the supplier",
];

/** The ring and voice bars from the logo, moving with whoever is talking. */
function Orb({ assistant }: { assistant: Assistant }) {
  const bars = useRef<(HTMLSpanElement | null)[]>([]);
  const live = assistant.state === "live";
  const levelRef = useRef(assistant.micLevel);
  levelRef.current = assistant.muted ? 0 : assistant.micLevel;
  const speakingRef = useRef(assistant.speaking);
  speakingRef.current = assistant.speaking;
  const outputRef = useRef(assistant.outputLevel);
  outputRef.current = assistant.outputLevel;

  useEffect(() => {
    if (!live) return;
    let frame = 0;
    let smooth = 0;
    const tick = (t: number) => {
      const target = speakingRef.current ? outputRef.current() : levelRef.current;
      smooth += (target - smooth) * 0.25;
      bars.current.forEach((el, i) => {
        if (!el) return;
        const wobble = 0.55 + 0.45 * Math.sin(t / 140 + i * 1.7);
        const center = 1 - Math.abs(i - 2) / 4;
        const h = 0.22 + Math.min(1, smooth * (0.5 + center) * (0.7 + 0.3 * wobble)) * 0.78;
        el.style.transform = `scaleY(${h.toFixed(3)})`;
      });
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      bars.current.forEach((el) => el && (el.style.transform = ""));
    };
  }, [live]);

  return (
    <div className={`orb is-${assistant.state} ${assistant.speaking ? "is-speaking" : ""}`} aria-hidden>
      <div className="orb-ring" />
      <div className="orb-bars">
        {[0, 1, 2, 3, 4].map((i) => (
          <span key={i} ref={(el) => {
              bars.current[i] = el;
            }} className="orb-bar" style={{ animationDelay: `${i * 90}ms` }} />
        ))}
      </div>
    </div>
  );
}

function FeedItem({ item }: { item: Item }) {
  if (item.role === "tool") {
    return (
      <div className={`feed-tool is-${item.status}`}>
        <span className="feed-tool-icon">{item.status === "running" ? <Spinner size={11} /> : item.status === "done" ? <Check size={12} strokeWidth={2.5} /> : <X size={12} strokeWidth={2.5} />}</span>
        <span className="feed-tool-text">{item.summary}</span>
      </div>
    );
  }
  return (
    <div className={`feed-msg is-${item.role}`}>
      <span className="feed-who">{item.role === "user" ? "You" : "Parrot"}</span>
      <p className="feed-text">{item.text || "…"}</p>
    </div>
  );
}

export function AssistantPanel({ assistant, onClose, hasKey, onSettings }: { assistant: Assistant; onClose: () => void; hasKey: boolean; onSettings: () => void }) {
  const { state, items } = assistant;
  const [text, setText] = useState("");
  const [queued, setQueued] = useState<string | null>(null);
  const feed = useRef<HTMLDivElement>(null);
  const live = state === "live";
  const busy = state === "connecting" || state === "ending";

  useEffect(() => {
    const el = feed.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [items]);

  useEffect(() => {
    if (live && queued) {
      assistant.send(queued);
      setQueued(null);
    }
  }, [live, queued, assistant]);

  const ask = (t: string) => {
    if (live) assistant.send(t);
    else {
      setQueued(t);
      void assistant.start();
    }
  };

  const status = state === "connecting" ? "Connecting…" : state === "ending" ? "Ending…" : live ? (assistant.speaking ? "Speaking" : assistant.muted ? "Muted" : "Listening") : "Ready";

  return (
    <aside className="assistant" aria-label="Assistant">
      <div className="assistant-head">
        <div>
          <h2 className="assistant-title">Assistant</h2>
          <p className="assistant-sub">Talks with your notes · Gemini Live</p>
        </div>
        <div className="assistant-head-actions">
          {items.length > 0 && !live && (
            <IconButton label="Clear conversation" onClick={assistant.clear}>
              <RotateCcw size={15} strokeWidth={1.75} />
            </IconButton>
          )}
          <IconButton label="Close assistant" onClick={onClose}>
            <PanelRightClose size={16} strokeWidth={1.75} />
          </IconButton>
        </div>
      </div>

      <div className="feed" ref={feed} aria-live="polite">
        {items.length === 0 ? (
          <div className="feed-empty">
            <p>Ask about any meeting, in Marathi, Hindi or English. Parrot reads your notes, and can add summaries, fix names or tidy up topics.</p>
            <div className="suggestions">
              {SUGGESTIONS.map((s) => (
                <button key={s} type="button" className="suggestion" disabled={!hasKey || busy} onClick={() => ask(s)}>
                  {s}
                </button>
              ))}
            </div>
          </div>
        ) : (
          items.map((item) => <FeedItem key={item.id} item={item} />)
        )}
      </div>

      {assistant.error && (
        <div className="assistant-error" role="alert">
          <span>{assistant.error}</span>
          <button type="button" className="link" onClick={assistant.clearError}>
            Dismiss
          </button>
        </div>
      )}

      <div className="assistant-foot">
        <div className="assistant-controls">
          <Orb assistant={assistant} />
          <div className="assistant-state">
            <span className="assistant-status">{status}</span>
            {!hasKey ? (
              <Button size="sm" variant="primary" onClick={onSettings}>
                Add API key
              </Button>
            ) : live || busy ? (
              <div className="assistant-buttons">
                <IconButton label={assistant.muted ? "Unmute" : "Mute"} active={assistant.muted} onClick={() => assistant.setMuted(!assistant.muted)} disabled={!live}>
                  {assistant.muted ? <MicOff size={16} strokeWidth={1.75} /> : <Mic size={16} strokeWidth={1.75} />}
                </IconButton>
                <Button size="sm" variant="danger" onClick={assistant.stop} disabled={state === "ending"}>
                  <PhoneOff size={14} strokeWidth={2} /> End
                </Button>
              </div>
            ) : (
              <Button variant="primary" onClick={() => void assistant.start()}>
                <Mic size={15} strokeWidth={2} /> Start talking
              </Button>
            )}
          </div>
        </div>
        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault();
            if (!text.trim()) return;
            ask(text);
            setText("");
          }}
        >
          <input className="composer-input" placeholder={live ? "Or type a message…" : "Type to start…"} value={text} onChange={(e) => setText(e.target.value)} disabled={!hasKey || busy} aria-label="Message the assistant" />
          <button type="submit" className="composer-send" aria-label="Send" disabled={!text.trim() || !hasKey || busy}>
            <ArrowUp size={15} strokeWidth={2.25} />
          </button>
        </form>
      </div>
    </aside>
  );
}
