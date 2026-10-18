import { CornerDownLeft, FileText, Folder } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import type { TreeNode } from "../lib/api";
import { rank } from "../lib/fuzzy";
import { flatten, parentOf } from "../lib/tree";
import { Modal } from "./primitives";

export interface Command {
  id: string;
  label: string;
  icon: ReactNode;
  hint?: string;
  run: () => void;
}

type Entry = { kind: "node"; node: TreeNode } | { kind: "command"; command: Command };

export function CommandPalette({ open, onClose, tree, commands, onOpen }: { open: boolean; onClose: () => void; tree: TreeNode[]; commands: Command[]; onOpen: (node: TreeNode) => void }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  const entries = useMemo<Entry[]>(() => {
    const nodes = flatten(tree);
    const q = query.trim();
    const cmds = rank(q, commands, (c) => c.label, 6).map((command) => ({ kind: "command" as const, command }));
    const found = q
      ? rank(q, nodes, (n) => n.path.replace(/\.md$/, "").replaceAll("/", " / "), 30)
      : nodes.filter((n) => n.kind === "note").sort((a, b) => b.modified - a.modified).slice(0, 8);
    const items = found.map((node) => ({ kind: "node" as const, node }));
    return q ? [...items, ...cmds] : [...cmds, ...items];
  }, [query, tree, commands]);

  const choose = (e: Entry | undefined) => {
    if (!e) return;
    onClose();
    if (e.kind === "node") onOpen(e.node);
    else e.command.run();
  };

  const close = () => {
    setQuery("");
    setActive(0);
    onClose();
  };

  return (
    <Modal open={open} onClose={close} width={580} labelledBy="palette-input">
      <div className="palette">
        <input
          id="palette-input"
          className="palette-input"
          placeholder="Search notes and topics, or run a command…"
          autoFocus
          value={query}
          role="combobox"
          aria-expanded
          aria-controls="palette-list"
          aria-activedescendant={`palette-${active}`}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((a) => Math.min(entries.length - 1, a + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(0, a - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              choose(entries[active]);
              setQuery("");
              setActive(0);
            }
          }}
        />
        <div className="palette-list" id="palette-list" role="listbox">
          {entries.length === 0 && <p className="palette-empty">No matches for “{query}”.</p>}
          {entries.map((e, i) => {
            const isNode = e.kind === "node";
            const label = isNode ? e.node.name : e.command.label;
            const sub = isNode ? parentOf(e.node.path).replaceAll("/", " / ") : e.command.hint;
            const icon = isNode ? e.node.kind === "topic" ? <Folder size={15} strokeWidth={1.75} /> : <FileText size={15} strokeWidth={1.75} /> : e.command.icon;
            return (
              <div
                key={isNode ? e.node.path : e.command.id}
                id={`palette-${i}`}
                role="option"
                aria-selected={i === active}
                className={`palette-row ${i === active ? "is-active" : ""}`}
                onMouseMove={() => setActive(i)}
                onClick={() => {
                  choose(e);
                  setQuery("");
                  setActive(0);
                }}
              >
                <span className="palette-icon">{icon}</span>
                <span className="palette-label">{label}</span>
                {sub && <span className="palette-sub">{sub}</span>}
                {i === active && <CornerDownLeft size={13} strokeWidth={2} className="palette-enter" />}
              </div>
            );
          })}
        </div>
      </div>
    </Modal>
  );
}
