import { AudioLines, FileText, Flag, Mic, RotateCcw, User, X } from "lucide-react";
import { type KeyboardEvent, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Author, type Version } from "../lib/api";
import { lineDiff, type Piece, type Row, stats } from "../lib/diff";
import { relativeTime } from "../lib/format";
import { parseFrontmatter } from "../lib/markdown";
import { useFeedback } from "./feedback";
import { Body } from "./NoteView";
import { Button, IconButton, Spinner } from "./primitives";

export const AUTHORS: Record<Author, { label: string; icon: ReactNode }> = {
  assistant: { label: "Parrot", icon: <AudioLines size={13} strokeWidth={2} /> },
  you: { label: "You", icon: <User size={13} strokeWidth={2} /> },
  recording: { label: "Recording", icon: <Mic size={13} strokeWidth={2} /> },
  restore: { label: "Restored", icon: <RotateCcw size={13} strokeWidth={2} /> },
  external: { label: "Outside Parrot", icon: <FileText size={13} strokeWidth={2} /> },
  original: { label: "Original", icon: <Flag size={13} strokeWidth={2} /> },
};

function dayLabel(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" });
}

const time = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
const fullTime = (ms: number) =>
  new Date(ms).toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", second: "2-digit" });

type Compare = "previous" | "current";
type Mode = "changes" | "text";

export function HistoryView({
  path,
  name,
  version,
  recording,
  focus,
  onClose,
}: {
  path: string;
  name: string;
  /** Bumped when the note changed on disk. */
  version: number;
  recording: boolean;
  /** A version to select first (e.g. the assistant's latest edit). */
  focus?: string;
  onClose: () => void;
}) {
  const fb = useFeedback();
  const [versions, setVersions] = useState<Version[] | null>(null);
  const [selected, setSelected] = useState<string | null>(focus ?? null);
  const [compare, setCompare] = useState<Compare>("previous");
  const [mode, setMode] = useState<Mode>("changes");
  const [current, setCurrent] = useState<string | null>(null);
  const [texts, setTexts] = useState<Record<string, string>>({});
  const [expanded, setExpanded] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const list = useRef<HTMLDivElement>(null);

  const load = useCallback(async () => {
    try {
      const [vs, doc] = await Promise.all([api.versions(path), api.read(path)]);
      setVersions(vs);
      setCurrent(doc.content);
      setSelected((s) => (s && vs.some((v) => v.id === s) ? s : (vs[0]?.id ?? null)));
    } catch (e) {
      fb.toast(e instanceof Error ? e.message : String(e), { tone: "error" });
    }
  }, [path, fb]);

  useEffect(() => {
    void load();
  }, [load, version]);

  const index = versions?.findIndex((v) => v.id === selected) ?? -1;
  const sel = index >= 0 ? versions?.[index] : undefined;
  const prev = index >= 0 ? versions?.[index + 1] : undefined;

  // Fetch the texts we need (cached; versions never change).
  useEffect(() => {
    const need = [sel?.id, compare === "previous" ? prev?.id : undefined].filter((id): id is string => !!id && !(id in texts));
    if (need.length === 0) return;
    let live = true;
    Promise.all(need.map((id) => api.version(path, id).then((r) => [id, r.content] as const)))
      .then((pairs) => live && setTexts((t) => ({ ...t, ...Object.fromEntries(pairs) })))
      .catch((e) => fb.toast(e instanceof Error ? e.message : String(e), { tone: "error" }));
    return () => {
      live = false;
    };
  }, [sel?.id, prev?.id, compare, path, texts, fb]);

  const selText = sel ? texts[sel.id] : undefined;
  const baseText = compare === "current" ? current : prev ? texts[prev.id] : "";
  const isCurrent = selText !== undefined && current !== null && selText === current;

  const rows = useMemo(() => {
    if (selText === undefined || baseText === undefined || baseText === null) return null;
    // "Compare with current" shows what restoring would change: current → this version.
    return lineDiff(baseText, selText, expanded ? 1e9 : 3);
  }, [selText, baseText, compare, expanded]);

  useEffect(() => setExpanded(false), [selected, compare]);

  const restore = async () => {
    if (!sel) return;
    const ok = await fb.confirm({
      title: "Restore this version?",
      body: (
        <>
          “{name}” will go back to how it was {relativeTime(sel.created)} ({fullTime(sel.created)}). The current text stays in the history, so you can
          always come back to it.
        </>
      ),
      confirm: "Restore",
    });
    if (!ok) return;
    setRestoring(true);
    try {
      await api.restoreVersion(path, sel.id);
      setSelected(null);
      await load();
      fb.toast("Restored. The previous text is saved in the history.");
    } catch (e) {
      fb.toast(e instanceof Error ? e.message : String(e), { tone: "error" });
    } finally {
      setRestoring(false);
    }
  };

  const onListKey = (e: KeyboardEvent) => {
    if (!versions || index < 0) return;
    if (e.key === "ArrowDown" || e.key === "j") {
      e.preventDefault();
      setSelected(versions[Math.min(versions.length - 1, index + 1)]?.id ?? null);
    } else if (e.key === "ArrowUp" || e.key === "k") {
      e.preventDefault();
      setSelected(versions[Math.max(0, index - 1)]?.id ?? null);
    }
  };

  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-id="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  // Group the timeline by day.
  const groups = useMemo(() => {
    const out: { day: string; items: Version[] }[] = [];
    for (const v of versions ?? []) {
      const day = dayLabel(v.created);
      const last = out[out.length - 1];
      if (last?.day === day) last.items.push(v);
      else out.push({ day, items: [v] });
    }
    return out;
  }, [versions]);

  return (
    <div className="history" onKeyDown={(e) => e.key === "Escape" && onClose()}>
      <header className="history-head">
        <div>
          <h1 className="history-title">History</h1>
          <p className="history-sub">
            {name}
            {versions && <span> · {versions.length === 1 ? "1 version" : `${versions.length} versions`}</span>}
          </p>
        </div>
        <IconButton label="Close history (Esc)" onClick={onClose}>
          <X size={16} strokeWidth={1.75} />
        </IconButton>
      </header>

      <div className="history-body">
        <div className="timeline" ref={list} role="listbox" aria-label="Versions" tabIndex={0} onKeyDown={onListKey}>
          {versions === null ? (
            <div className="timeline-empty">
              <Spinner />
            </div>
          ) : versions.length === 0 ? (
            <p className="timeline-empty">No versions yet. Every change from now on is kept here.</p>
          ) : (
            groups.map((g) => (
              <section key={g.day} className="timeline-day">
                <h2 className="timeline-label">{g.day}</h2>
                {g.items.map((v) => {
                  const a = AUTHORS[v.author];
                  const latest = v.id === versions[0]?.id;
                  return (
                    <button
                      key={v.id}
                      type="button"
                      role="option"
                      data-id={v.id}
                      aria-selected={v.id === selected}
                      className={`version is-${v.author} ${v.id === selected ? "is-selected" : ""}`}
                      onClick={() => setSelected(v.id)}
                    >
                      <span className="version-dot" aria-hidden>
                        {a.icon}
                      </span>
                      <span className="version-main">
                        <span className="version-line">
                          <span className="version-who">{a.label}</span>
                          {latest && <span className="version-current">Current</span>}
                          <time className="version-time" title={fullTime(v.created)}>
                            {time(v.created)}
                          </time>
                        </span>
                        <span className="version-what">{v.summary}</span>
                        {(v.added > 0 || v.removed > 0) && (
                          <span className="version-stats">
                            {v.added > 0 && <span className="plus">+{v.added}</span>}
                            {v.removed > 0 && <span className="minus">−{v.removed}</span>}
                          </span>
                        )}
                      </span>
                    </button>
                  );
                })}
              </section>
            ))
          )}
        </div>

        <div className="preview">
          {sel ? (
            <>
              <div className="preview-head">
                <div className="preview-meta">
                  <span className={`author-chip is-${sel.author}`}>
                    {AUTHORS[sel.author].icon}
                    {AUTHORS[sel.author].label}
                  </span>
                  <span className="preview-what">{sel.summary}</span>
                  <span className="preview-when">{fullTime(sel.created)}</span>
                </div>
                <div className="preview-actions">
                  {isCurrent ? (
                    <span className="preview-note">This is the current text</span>
                  ) : (
                    <Button size="sm" variant="primary" onClick={() => void restore()} disabled={restoring || recording} title={recording ? "Stop the recording first" : undefined}>
                      {restoring ? <Spinner size={12} /> : <RotateCcw size={13} strokeWidth={2} />} Restore this version
                    </Button>
                  )}
                </div>
              </div>

              <div className="preview-bar">
                <div className="segmented" role="radiogroup" aria-label="View">
                  <button type="button" role="radio" aria-checked={mode === "changes"} className={mode === "changes" ? "is-on" : ""} onClick={() => setMode("changes")}>
                    Changes
                  </button>
                  <button type="button" role="radio" aria-checked={mode === "text"} className={mode === "text" ? "is-on" : ""} onClick={() => setMode("text")}>
                    Full text
                  </button>
                </div>
                {mode === "changes" && (
                  <div className="segmented" role="radiogroup" aria-label="Compare with">
                    <button type="button" role="radio" aria-checked={compare === "previous"} className={compare === "previous" ? "is-on" : ""} onClick={() => setCompare("previous")}>
                      What changed
                    </button>
                    <button type="button" role="radio" aria-checked={compare === "current"} className={compare === "current" ? "is-on" : ""} onClick={() => setCompare("current")}>
                      vs. current
                    </button>
                  </div>
                )}
              </div>

              <div className="preview-scroll">
                {selText === undefined ? (
                  <div className="timeline-empty">
                    <Spinner />
                  </div>
                ) : mode === "text" ? (
                  <div className="doc preview-doc">
                    <Body body={parseFrontmatter(selText).body} empty="This version is empty." onEdit={() => {}} />
                  </div>
                ) : rows ? (
                  <DiffView rows={rows} compare={compare} hasPrevious={!!prev} onExpand={() => setExpanded(true)} />
                ) : (
                  <div className="timeline-empty">
                    <Spinner />
                  </div>
                )}
              </div>
            </>
          ) : (
            versions && versions.length > 0 && <p className="timeline-empty">Pick a version on the left.</p>
          )}
        </div>
      </div>
    </div>
  );
}

function Pieces({ pieces }: { pieces: Piece[] }) {
  return (
    <>
      {pieces.map((p, i) =>
        p.changed ? (
          <mark key={i} className="word">
            {p.text}
          </mark>
        ) : (
          <span key={i}>{p.text}</span>
        ),
      )}
    </>
  );
}

function DiffView({ rows, compare, hasPrevious, onExpand }: { rows: Row[]; compare: Compare; hasPrevious: boolean; onExpand: () => void }) {
  const { added, removed } = stats(rows);
  if (added === 0 && removed === 0) {
    return <p className="diff-same">{compare === "current" ? "Same as the current text." : "No text changes in this version."}</p>;
  }
  return (
    <div className="diff-wrap">
      <p className="diff-summary">
        {compare === "current" ? "Restoring would change the current text like this" : hasPrevious ? "Changed from the version before" : "The first version"}
        <span className="plus">+{added}</span>
        <span className="minus">−{removed}</span>
      </p>
      <div className="diff" role="table" aria-label="Changes">
        {rows.map((r, i) => {
          if (r.kind === "skip") {
            return (
              <button key={i} type="button" className="d-skip" onClick={onExpand}>
                ⋯ {r.count} unchanged {r.count === 1 ? "line" : "lines"}
              </button>
            );
          }
          if (r.kind === "equal") {
            return (
              <div key={i} className="d-row" role="row">
                <span className="d-num">{r.b + 1}</span>
                <span className="d-sign" />
                <span className="d-text">{r.text || " "}</span>
              </div>
            );
          }
          const del = r.kind === "delete";
          return (
            <div key={i} className={`d-row ${del ? "is-del" : "is-ins"}`} role="row">
              <span className="d-num">{del ? r.a + 1 : r.b + 1}</span>
              <span className="d-sign" aria-label={del ? "removed" : "added"}>
                {del ? "−" : "+"}
              </span>
              <span className="d-text">{r.pieces.every((p) => !p.text) ? " " : <Pieces pieces={r.pieces} />}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
