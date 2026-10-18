import { FilePlus2, FileText, Folder, FolderPlus, Mic } from "lucide-react";
import { useEffect, useState } from "react";
import type { TreeNode } from "../lib/api";
import { plural, relativeTime } from "../lib/format";
import { noteCount, parentOf, recentNotes } from "../lib/tree";
import { Button } from "./primitives";

export function TopicView({
  topic,
  onOpen,
  onRename,
  onRecord,
  onNewNote,
  onNewTopic,
}: {
  topic: TreeNode;
  onOpen: (node: TreeNode) => void;
  onRename: (path: string, name: string) => Promise<void>;
  onRecord: (topic: string) => void;
  onNewNote: (topic: string) => void;
  onNewTopic: (parent: string) => void;
}) {
  const children = topic.children ?? [];
  const subtopics = children.filter((c) => c.kind === "topic");
  const notes = children.filter((c) => c.kind === "note");
  const [name, setName] = useState(topic.name);
  useEffect(() => setName(topic.name), [topic.name]);

  return (
    <div className="doc-scroll">
      <div className="doc">
        <header className="doc-head">
          <input
            className="doc-title"
            aria-label="Topic name"
            value={name}
            maxLength={120}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => {
              const v = name.trim();
              if (v && v !== topic.name) void onRename(topic.path, v).catch(() => setName(topic.name));
              else setName(topic.name);
            }}
            onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
          />
          <p className="doc-meta">
            <span className="badge">Topic</span>
            <span>{plural(noteCount(topic), "note")}</span>
            {subtopics.length > 0 && <span>{plural(subtopics.length, "subtopic")}</span>}
          </p>
        </header>

        <div className="actions-row">
          <Button variant="primary" onClick={() => onRecord(topic.path)}>
            <Mic size={15} strokeWidth={2} /> Record here
          </Button>
          <Button onClick={() => onNewNote(topic.path)}>
            <FilePlus2 size={15} strokeWidth={1.75} /> New note
          </Button>
          <Button onClick={() => onNewTopic(topic.path)}>
            <FolderPlus size={15} strokeWidth={1.75} /> New subtopic
          </Button>
        </div>

        {subtopics.length > 0 && (
          <section className="list-section">
            <h3 className="list-heading">Subtopics</h3>
            <ul className="list">
              {subtopics.map((t) => (
                <li key={t.path}>
                  <button type="button" className="list-row" onClick={() => onOpen(t)}>
                    <Folder size={15} strokeWidth={1.75} className="list-icon" />
                    <span className="list-name">{t.name}</span>
                    <span className="list-meta">{plural(noteCount(t), "note")}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}

        <section className="list-section">
          <h3 className="list-heading">Notes</h3>
          {notes.length === 0 ? (
            <p className="muted">No notes in {topic.name} yet. Record a meeting or write a note.</p>
          ) : (
            <ul className="list">
              {notes.map((n) => (
                <li key={n.path}>
                  <button type="button" className="list-row" onClick={() => onOpen(n)}>
                    <FileText size={15} strokeWidth={1.75} className="list-icon" />
                    <span className="list-name">{n.name}</span>
                    <span className="list-meta">{relativeTime(n.modified)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}

export function Home({
  tree,
  loaded,
  onOpen,
  onRecord,
  onNewTopic,
  hasKey,
  onSettings,
}: {
  tree: TreeNode[];
  loaded: boolean;
  onOpen: (node: TreeNode) => void;
  onRecord: () => void;
  onNewTopic: () => void;
  hasKey: boolean;
  onSettings: () => void;
}) {
  const recent = recentNotes(tree, 10);
  const today = new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  return (
    <div className="doc-scroll">
      <div className="doc">
        <header className="doc-head">
          <p className="eyebrow">{today}</p>
          <h1 className="doc-title is-static">Your notebook</h1>
        </header>

        {!hasKey && (
          <div className="banner">
            <span>Parrot needs a Gemini API key to record and talk.</span>
            <div className="banner-actions">
              <Button size="sm" variant="primary" onClick={onSettings}>
                Add key
              </Button>
            </div>
          </div>
        )}

        <div className="start-grid">
          <button type="button" className="start-card" onClick={onRecord}>
            <span className="start-icon is-rec">
              <Mic size={16} strokeWidth={2} />
            </span>
            <span className="start-title">Record a meeting</span>
            <span className="start-text">Live transcript in Marathi, Hindi and English, saved as you go.</span>
          </button>
          <button type="button" className="start-card" onClick={onNewTopic}>
            <span className="start-icon">
              <FolderPlus size={16} strokeWidth={1.75} />
            </span>
            <span className="start-title">Create a topic</span>
            <span className="start-text">Group meetings by team, client or project, with subtopics inside.</span>
          </button>
        </div>

        <section className="list-section">
          <h3 className="list-heading">Recent</h3>
          {loaded && recent.length === 0 ? (
            <p className="muted">Nothing here yet. Your recordings and notes will show up here.</p>
          ) : (
            <ul className="list">
              {recent.map((n) => (
                <li key={n.path}>
                  <button type="button" className="list-row" onClick={() => onOpen(n)}>
                    <FileText size={15} strokeWidth={1.75} className="list-icon" />
                    <span className="list-name">{n.name}</span>
                    <span className="list-path">{parentOf(n.path).replaceAll("/", " / ")}</span>
                    <span className="list-meta">{relativeTime(n.modified)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}
