import { Square } from "lucide-react";
import { useEffect, useState } from "react";
import { clock } from "../lib/format";
import { basename } from "../lib/tree";
import type { Recorder } from "../lib/useRecorder";
import { Spinner } from "./primitives";

/** Five bars following the microphone level. */
export function Meter({ level, bars = 5 }: { level: number; bars?: number }) {
  return (
    <span className="meter" aria-hidden>
      {Array.from({ length: bars }, (_, i) => {
        const center = 1 - Math.abs(i - (bars - 1) / 2) / bars;
        const h = 0.18 + Math.min(1, level * (0.6 + center)) * 0.82;
        return <span key={i} className="meter-bar" style={{ transform: `scaleY(${h.toFixed(3)})` }} />;
      })}
    </span>
  );
}

export function RecordDock({ recorder, onShow, showing }: { recorder: Recorder; onShow: () => void; showing: boolean }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(t);
  }, []);
  const elapsed = recorder.startedAt ? (now - recorder.startedAt) / 1000 : 0;
  const status =
    recorder.state === "starting"
      ? "Connecting…"
      : recorder.state === "reconnecting"
        ? "Reconnecting…"
        : recorder.state === "stopping"
          ? "Saving the last words…"
          : "Recording";
  return (
    <div className="dock" role="region" aria-label="Recording">
      <span className={`rec-dot ${recorder.state === "recording" ? "is-pulsing" : ""}`} />
      <span className="dock-time">{clock(elapsed)}</span>
      <Meter level={recorder.level} />
      <div className="dock-info">
        <span className="dock-status">{status}</span>
        {!showing && recorder.path && (
          <button type="button" className="dock-note" onClick={onShow}>
            {basename(recorder.path).replace(/\.md$/, "")}
          </button>
        )}
      </div>
      <button type="button" className="dock-stop" onClick={recorder.stop} disabled={recorder.state === "stopping" || recorder.state === "starting"}>
        {recorder.state === "stopping" ? <Spinner size={12} /> : <Square size={11} strokeWidth={0} fill="currentColor" />}
        <span>Stop</span>
      </button>
    </div>
  );
}
