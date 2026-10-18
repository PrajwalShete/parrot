import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ApiError, api, type Doc } from "../lib/api";
import { longDate, plural } from "../lib/format";
import { blocks, countLines, parseFrontmatter, render, spanSeconds } from "../lib/markdown";
import type { Recorder } from "../lib/useRecorder";
import { useFeedback } from "./feedback";
import { Button, Spinner } from "./primitives";

type SaveState = "saved" | "dirty" | "saving" | "conflict" | "error";

export function NoteView({
  path,
  version,
  recorder,
  editing,
  setEditing,
  onRename,
  onMissing,
}: {
  path: string;
  /** Bumped whenever this note changed on disk. */
  version: number;
  recorder: Recorder;
  editing: boolean;
  setEditing: (v: boolean) => void;
  onRename: (path: string, name: string) => Promise<void>;
  onMissing: () => void;
}) {
  const fb = useFeedback();
  const [doc, setDoc] = useState<Doc | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [save, setSave] = useState<SaveState>("saved");
  const recordingHere = recorder.path === path && recorder.state !== "idle";
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const docRef = useRef(doc);
  docRef.current = doc;
  const saveRef = useRef(save);
  saveRef.current = save;

  const load = useCallback(async () => {
    try {
      const d = await api.read(path);
      setDoc(d);
      setError(null);
      if (saveRef.current === "saved") setDraft(parseFrontmatter(d.content).body);
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) onMissing();
      else setError(e instanceof Error ? e.message : String(e));
    }
  }, [path, onMissing]);

  useEffect(() => {
    setDoc(null);
    setSave("saved");
    stick.current = true;
  }, [path]);

  useEffect(() => {
    void load();
  }, [load, version]);

  // ---- Editing: autosave with conflict detection ------------------------------------------------

  const parsed = useMemo(() => (doc ? parseFrontmatter(doc.content) : null), [doc]);

  const persist = useCallback(
    async (body: string, force = false) => {
      const current = docRef.current;
      if (!current) return;
      const head = current.content.slice(0, current.content.length - parseFrontmatter(current.content).body.length);
      setSave("saving");
      try {
        const saved = await api.write(path, head + body, force ? undefined : current.modified);
        setDoc(saved);
        setSave("saved");
      } catch (e) {
        if (e instanceof ApiError && e.status === 409) setSave("conflict");
        else {
          setSave("error");
          fb.toast(e instanceof Error ? e.message : "Couldn't save", { tone: "error" });
        }
      }
    },
    [path, fb],
  );

  useEffect(() => {
    if (save !== "dirty") return;
    const t = window.setTimeout(() => void persist(draft), 700);
    return () => window.clearTimeout(t);
  }, [draft, save, persist]);

  // Save before leaving edit mode or the note.
  useEffect(() => {
    if (!editing && saveRef.current === "dirty") void persist(draft);
  }, [editing]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---- Follow the live transcript ------------------------------------------------------------------

  const onScroll = () => {
    const el = scroller.current;
    if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
  };

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && recordingHere && stick.current) el.scrollTop = el.scrollHeight;
  }, [doc, recorder.interim, recordingHere]);

  if (error) {
    return (
      <div className="doc-state">
        <p>{error}</p>
        <Button onClick={() => void load()}>Try again</Button>
      </div>
    );
  }
  if (!doc || !parsed) {
    return (
      <div className="doc-state" aria-busy>
        <Spinner />
      </div>
    );
  }

  const isTranscript = parsed.meta.type === "transcript";
  const lines = countLines(parsed.body);
  const span = spanSeconds(parsed.body);
  const created = parsed.meta.created ? Date.parse(parsed.meta.created) : NaN;

  return (
    <div className="doc-scroll" ref={scroller} onScroll={onScroll}>
      <article className="doc">
        <header className="doc-head">
          <TitleField key={doc.path} name={doc.name} onRename={(n) => onRename(doc.path, n)} />
          <p className="doc-meta">
            <span className={`badge ${isTranscript ? "badge-transcript" : ""}`}>{isTranscript ? "Transcript" : "Note"}</span>
            <span>{longDate(Number.isNaN(created) ? doc.modified : created)}</span>
            {lines > 0 && <span>{plural(lines, "line")}</span>}
            {span > 59 && <span>{Math.round(span / 60)} min</span>}
            {editing && <SaveLabel state={save} />}
          </p>
        </header>

        {save === "conflict" && (
          <div className="banner" role="alert">
            <span>This note changed while you were editing (a recording or the assistant wrote to it).</span>
            <div className="banner-actions">
              <Button
                size="sm"
                onClick={() => {
                  setSave("saved");
                  void load().then(() => setDraft(parseFrontmatter(docRef.current?.content ?? "").body));
                }}
              >
                Load theirs
              </Button>
              <Button size="sm" variant="primary" onClick={() => void persist(draft, true)}>
                Keep mine
              </Button>
            </div>
          </div>
        )}

        {editing ? (
          <Editor
            value={draft}
            onChange={(v) => {
              setDraft(v);
              setSave("dirty");
            }}
            onDone={() => setEditing(false)}
          />
        ) : (
          <Body body={parsed.body} empty={recordingHere ? "Listening… lines appear here as people finish speaking." : isTranscript ? "Nothing recorded yet. Press Record to start." : "This note is empty. Press E to write."} onEdit={() => setEditing(true)} />
        )}

        {recordingHere && !editing && (
          <div className="live-line" aria-live="polite">
            <span className="live-time">
              <span className="rec-dot" />
            </span>
            <span className={`live-text ${recorder.interim ? "" : "is-waiting"}`}>{recorder.interim || (recorder.state === "reconnecting" ? "Reconnecting…" : "Listening…")}</span>
          </div>
        )}
      </article>
    </div>
  );
}

function SaveLabel({ state }: { state: SaveState }) {
  const text = { saved: "Saved", dirty: "Editing", saving: "Saving…", conflict: "Not saved", error: "Not saved" }[state];
  return <span className={`save-state is-${state}`}>{text}</span>;
}

function TitleField({ name, onRename }: { name: string; onRename: (name: string) => Promise<void> }) {
  const [value, setValue] = useState(name);
  useEffect(() => setValue(name), [name]);
  const commit = () => {
    const v = value.trim();
    if (v && v !== name) void onRename(v).catch(() => setValue(name));
    else setValue(name);
  };
  return (
    <input
      className="doc-title"
      aria-label="Note title"
      value={value}
      maxLength={120}
      onChange={(e) => setValue(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setValue(name);
          requestAnimationFrame(() => (e.target as HTMLInputElement).blur());
        }
      }}
    />
  );
}

function Body({ body, empty, onEdit }: { body: string; empty: string; onEdit: () => void }) {
  const parts = useMemo(() => blocks(body), [body]);
  if (parts.length === 0) {
    return (
      <p className="doc-empty" onDoubleClick={onEdit}>
        {empty}
      </p>
    );
  }
  return (
    <div className="doc-body" onDoubleClick={onEdit}>
      {parts.map((b, i) =>
        b.kind === "line" ? (
          <div key={i} className="t-line">
            <time className="t-time">{b.time.slice(0, 5)}</time>
            <p className="t-text">{b.text}</p>
          </div>
        ) : (
          <div key={i} className="prose" dangerouslySetInnerHTML={{ __html: render(b.text) }} />
        ),
      )}
    </div>
  );
}

function Editor({ value, onChange, onDone }: { value: string; onChange: (v: string) => void; onDone: () => void }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${el.scrollHeight + 2}px`;
  }, [value]);
  useEffect(() => ref.current?.focus(), []);
  return (
    <textarea
      ref={ref}
      className="editor"
      aria-label="Note text (Markdown)"
      spellCheck
      value={value}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Escape" || (e.key === "Enter" && (e.metaKey || e.ctrlKey))) {
          e.preventDefault();
          onDone();
        }
      }}
      placeholder="Write in Markdown…"
    />
  );
}
