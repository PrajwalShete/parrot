import { FolderPlus, HardDrive, Mic, PanelLeftClose, Search, Settings } from "lucide-react";
import type { TreeNode } from "../lib/api";
import { Logo } from "./Logo";
import { IconButton, Kbd, mod } from "./primitives";
import { Tree, type TreeActions } from "./Tree";

export function Sidebar({
  tree,
  loaded,
  selected,
  expanded,
  onToggle,
  recording,
  actions,
  hasKey,
  folder,
  onSearch,
  onRecord,
  onNewTopic,
  onSettings,
  onReveal,
  onCollapse,
}: {
  tree: TreeNode[];
  loaded: boolean;
  selected: string | null;
  expanded: Set<string>;
  onToggle: (path: string, open?: boolean) => void;
  recording: string | null;
  actions: TreeActions;
  hasKey: boolean;
  folder: string;
  onSearch: () => void;
  onRecord: () => void;
  onNewTopic: () => void;
  onSettings: () => void;
  onReveal: () => void;
  onCollapse: () => void;
}) {
  return (
    <aside className="sidebar" aria-label="Notebook">
      <div className="sidebar-head">
        <a href="#/" className="brand" aria-label="Parrot home">
          <Logo size={20} live={!!recording} />
          <span className="brand-name">parrot</span>
        </a>
        <IconButton label={`Hide sidebar (${mod}\\)`} onClick={onCollapse}>
          <PanelLeftClose size={16} strokeWidth={1.75} />
        </IconButton>
      </div>

      <button type="button" className="search-trigger" onClick={onSearch}>
        <Search size={14} strokeWidth={2} />
        <span>Search</span>
        <Kbd>{mod}K</Kbd>
      </button>

      <button type="button" className="record-cta" onClick={onRecord} disabled={!!recording}>
        <Mic size={15} strokeWidth={2} />
        <span>{recording ? "Recording…" : "New recording"}</span>
        {recording && <span className="rec-dot" />}
      </button>

      <div className="sidebar-section">
        <span className="sidebar-label">Topics</span>
        <IconButton label="New topic" onClick={onNewTopic}>
          <FolderPlus size={15} strokeWidth={1.75} />
        </IconButton>
      </div>

      <div className="sidebar-scroll">
        {loaded && tree.length === 0 ? (
          <div className="sidebar-empty">
            <p>No topics yet.</p>
            <button type="button" className="link" onClick={onNewTopic}>
              Create your first topic
            </button>
          </div>
        ) : (
          <Tree nodes={tree} selected={selected} expanded={expanded} onToggle={onToggle} recording={recording} actions={actions} />
        )}
      </div>

      <div className="sidebar-foot">
        {!hasKey && (
          <button type="button" className="key-warning" onClick={onSettings}>
            Add your Gemini API key to record
          </button>
        )}
        <button type="button" className="foot-row" onClick={onReveal} title={folder}>
          <HardDrive size={15} strokeWidth={1.75} />
          <span className="foot-text">{folder}</span>
        </button>
        <button type="button" className="foot-row" onClick={onSettings}>
          <Settings size={15} strokeWidth={1.75} />
          <span>Settings</span>
        </button>
      </div>
    </aside>
  );
}
