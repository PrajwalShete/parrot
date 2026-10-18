import { ChevronRight, FileText, Folder, FolderOpen, MoreHorizontal, Plus } from "lucide-react";
import { type DragEvent, type KeyboardEvent, useMemo, useRef, useState } from "react";
import type { TreeNode } from "../lib/api";
import { parentOf } from "../lib/tree";

export interface TreeActions {
  open: (node: TreeNode) => void;
  menu: (node: TreeNode, at: { x: number; y: number }) => void;
  addMenu: (topic: TreeNode, at: { x: number; y: number }) => void;
  rename: (node: TreeNode) => void;
  remove: (node: TreeNode) => void;
  drop: (from: string, toTopic: string) => void;
}

interface Row {
  node: TreeNode;
  depth: number;
}

const DRAG_TYPE = "application/x-parrot-path";

export function Tree({
  nodes,
  selected,
  expanded,
  onToggle,
  recording,
  actions,
}: {
  nodes: TreeNode[];
  selected: string | null;
  expanded: Set<string>;
  onToggle: (path: string, open?: boolean) => void;
  recording: string | null;
  actions: TreeActions;
}) {
  const rows = useMemo(() => {
    const out: Row[] = [];
    const walk = (list: TreeNode[], depth: number) => {
      for (const node of list) {
        out.push({ node, depth });
        if (node.kind === "topic" && expanded.has(node.path)) walk(node.children ?? [], depth + 1);
      }
    };
    walk(nodes, 0);
    return out;
  }, [nodes, expanded]);

  const [focus, setFocus] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const refs = useRef(new Map<string, HTMLDivElement>());
  const focused = rows.some((r) => r.node.path === focus) ? focus : (selected ?? rows[0]?.node.path ?? null);

  const moveFocus = (path: string | undefined) => {
    if (!path) return;
    setFocus(path);
    refs.current.get(path)?.focus();
  };

  const onKey = (e: KeyboardEvent, row: Row, i: number) => {
    const { node } = row;
    const isTopic = node.kind === "topic";
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        moveFocus(rows[i + 1]?.node.path);
        break;
      case "ArrowUp":
        e.preventDefault();
        moveFocus(rows[i - 1]?.node.path);
        break;
      case "ArrowRight":
        e.preventDefault();
        if (isTopic && !expanded.has(node.path)) onToggle(node.path, true);
        else if (isTopic) moveFocus(rows[i + 1]?.depth === row.depth + 1 ? rows[i + 1]?.node.path : undefined);
        break;
      case "ArrowLeft":
        e.preventDefault();
        if (isTopic && expanded.has(node.path)) onToggle(node.path, false);
        else moveFocus(parentOf(node.path) || undefined);
        break;
      case "Home":
        e.preventDefault();
        moveFocus(rows[0]?.node.path);
        break;
      case "End":
        e.preventDefault();
        moveFocus(rows[rows.length - 1]?.node.path);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        actions.open(node);
        if (isTopic) onToggle(node.path, true);
        break;
      case "F2":
        e.preventDefault();
        actions.rename(node);
        break;
      case "Delete":
      case "Backspace":
        if (e.metaKey || e.key === "Delete") {
          e.preventDefault();
          actions.remove(node);
        }
        break;
      case "ContextMenu": {
        e.preventDefault();
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        actions.menu(node, { x: r.left + 24, y: r.bottom });
        break;
      }
    }
  };

  const allowDrop = (e: DragEvent, topic: string) => {
    if (!e.dataTransfer.types.includes(DRAG_TYPE)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDropTarget(topic);
  };

  const onDrop = (e: DragEvent, topic: string) => {
    const from = e.dataTransfer.getData(DRAG_TYPE);
    setDropTarget(null);
    if (!from) return;
    e.preventDefault();
    if (from === topic || topic.startsWith(from + "/") || parentOf(from) === topic) return;
    actions.drop(from, topic);
  };

  return (
    <div
      className={`tree ${dropTarget === "" ? "is-drop-root" : ""}`}
      role="tree"
      aria-label="Topics and notes"
      onDragOver={(e) => allowDrop(e, "")}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) setDropTarget(null);
      }}
      onDrop={(e) => onDrop(e, "")}
    >
      {rows.map((row, i) => {
        const { node, depth } = row;
        const isTopic = node.kind === "topic";
        const open = isTopic && expanded.has(node.path);
        const isSelected = node.path === selected;
        return (
          <div
            key={node.path}
            ref={(el) => {
              if (el) refs.current.set(node.path, el);
              else refs.current.delete(node.path);
            }}
            role="treeitem"
            aria-level={depth + 1}
            aria-expanded={isTopic ? open : undefined}
            aria-selected={isSelected}
            tabIndex={node.path === focused ? 0 : -1}
            className={`tree-row ${isSelected ? "is-selected" : ""} ${dropTarget === node.path ? "is-drop" : ""}`}
            style={{ paddingLeft: 8 + depth * 14 }}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(DRAG_TYPE, node.path);
              e.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={isTopic ? (e) => (e.stopPropagation(), allowDrop(e, node.path)) : undefined}
            onDrop={isTopic ? (e) => (e.stopPropagation(), onDrop(e, node.path)) : undefined}
            onDragEnd={() => setDropTarget(null)}
            onFocus={() => setFocus(node.path)}
            onKeyDown={(e) => onKey(e, row, i)}
            onClick={() => {
              actions.open(node);
              if (isTopic && !open) onToggle(node.path, true);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              actions.menu(node, { x: e.clientX, y: e.clientY });
            }}
          >
            {isTopic ? (
              <span
                className={`tree-chevron ${open ? "is-open" : ""}`}
                onClick={(e) => {
                  e.stopPropagation();
                  onToggle(node.path);
                }}
                aria-hidden
              >
                <ChevronRight size={13} strokeWidth={2} />
              </span>
            ) : (
              <span className="tree-chevron" aria-hidden />
            )}
            <span className="tree-icon" aria-hidden>
              {isTopic ? (
                open ? (
                  <FolderOpen size={15} strokeWidth={1.75} />
                ) : (
                  <Folder size={15} strokeWidth={1.75} />
                )
              ) : recording === node.path ? (
                <span className="rec-dot" />
              ) : (
                <FileText size={15} strokeWidth={1.75} />
              )}
            </span>
            <span className="tree-name">{node.name}</span>
            <span className="tree-actions">
              {isTopic && (
                <button
                  type="button"
                  tabIndex={-1}
                  aria-label={`Add to ${node.name}`}
                  title="Add"
                  className="tree-action"
                  onClick={(e) => {
                    e.stopPropagation();
                    const r = e.currentTarget.getBoundingClientRect();
                    actions.addMenu(node, { x: r.left, y: r.bottom + 4 });
                  }}
                >
                  <Plus size={14} strokeWidth={2} />
                </button>
              )}
              <button
                type="button"
                tabIndex={-1}
                aria-label={`More for ${node.name}`}
                title="More"
                className="tree-action"
                onClick={(e) => {
                  e.stopPropagation();
                  const r = e.currentTarget.getBoundingClientRect();
                  actions.menu(node, { x: r.left, y: r.bottom + 4 });
                }}
              >
                <MoreHorizontal size={14} strokeWidth={2} />
              </button>
            </span>
          </div>
        );
      })}
    </div>
  );
}
