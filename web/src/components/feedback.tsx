// Toasts and promise-based dialogs (prompt, confirm, pick a topic), shared through context so any
// component can `await ask.prompt(...)`.

import { createContext, type ReactNode, useCallback, useContext, useMemo, useRef, useState } from "react";
import { Folder, FolderRoot } from "lucide-react";
import type { TreeNode } from "../lib/api";
import { topics } from "../lib/tree";
import { Button, Modal } from "./primitives";

// ---- Toasts -------------------------------------------------------------------------------------

interface Toast {
  id: number;
  text: string;
  tone: "info" | "error";
  action?: { label: string; run: () => void };
}

interface Feedback {
  toast: (text: string, opts?: { tone?: "info" | "error"; action?: Toast["action"] }) => void;
  prompt: (opts: { title: string; label: string; value?: string; confirm?: string; select?: "all" | "stem" }) => Promise<string | null>;
  confirm: (opts: { title: string; body: ReactNode; confirm: string; danger?: boolean }) => Promise<boolean>;
  pickTopic: (opts: { title: string; tree: TreeNode[]; exclude?: string; current?: string; confirm: string }) => Promise<string | null>;
}

const Ctx = createContext<Feedback | null>(null);

export const useFeedback = () => {
  const v = useContext(Ctx);
  if (!v) throw new Error("useFeedback outside FeedbackProvider");
  return v;
};

type Pending =
  | { kind: "prompt"; title: string; label: string; value: string; confirm: string; select: "all" | "stem"; resolve: (v: string | null) => void }
  | { kind: "confirm"; title: string; body: ReactNode; confirm: string; danger: boolean; resolve: (v: boolean) => void }
  | { kind: "pick"; title: string; tree: TreeNode[]; exclude?: string; current?: string; confirm: string; resolve: (v: string | null) => void };

let toastId = 1;

export function FeedbackProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [pending, setPending] = useState<Pending | null>(null);
  const timers = useRef(new Map<number, number>());

  const dismiss = useCallback((id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
    window.clearTimeout(timers.current.get(id));
    timers.current.delete(id);
  }, []);

  const toast = useCallback<Feedback["toast"]>(
    (text, opts) => {
      const id = toastId++;
      setToasts((t) => [...t.slice(-2), { id, text, tone: opts?.tone ?? "info", action: opts?.action }]);
      timers.current.set(id, window.setTimeout(() => dismiss(id), opts?.action ? 7000 : 4000));
    },
    [dismiss],
  );

  const value = useMemo<Feedback>(
    () => ({
      toast,
      prompt: (o) => new Promise((resolve) => setPending({ kind: "prompt", value: "", confirm: "Save", select: "all", ...o, resolve })),
      confirm: (o) => new Promise((resolve) => setPending({ kind: "confirm", danger: false, ...o, resolve })),
      pickTopic: (o) => new Promise((resolve) => setPending({ kind: "pick", ...o, resolve })),
    }),
    [toast],
  );

  const close = () => setPending(null);

  return (
    <Ctx.Provider value={value}>
      {children}
      {pending?.kind === "prompt" && (
        <PromptDialog
          {...pending}
          onDone={(v) => {
            pending.resolve(v);
            close();
          }}
        />
      )}
      {pending?.kind === "confirm" && (
        <Modal
          open
          title={pending.title}
          onClose={() => {
            pending.resolve(false);
            close();
          }}
          footer={
            <>
              <Button
                onClick={() => {
                  pending.resolve(false);
                  close();
                }}
              >
                Cancel
              </Button>
              <Button
                autoFocus
                variant={pending.danger ? "danger" : "primary"}
                onClick={() => {
                  pending.resolve(true);
                  close();
                }}
              >
                {pending.confirm}
              </Button>
            </>
          }
        >
          <div className="modal-text">{pending.body}</div>
        </Modal>
      )}
      {pending?.kind === "pick" && (
        <TopicPicker
          {...pending}
          onDone={(v) => {
            pending.resolve(v);
            close();
          }}
        />
      )}
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone === "error" ? "is-error" : ""}`}>
            <span>{t.text}</span>
            {t.action && (
              <button
                type="button"
                className="toast-action"
                onClick={() => {
                  t.action?.run();
                  dismiss(t.id);
                }}
              >
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>
    </Ctx.Provider>
  );
}

function PromptDialog({
  title,
  label,
  value,
  confirm,
  select,
  onDone,
}: {
  title: string;
  label: string;
  value: string;
  confirm: string;
  select: "all" | "stem";
  onDone: (v: string | null) => void;
}) {
  const [text, setText] = useState(value);
  const input = useRef<HTMLInputElement>(null);
  return (
    <Modal
      open
      title={title}
      onClose={() => onDone(null)}
      footer={
        <>
          <Button onClick={() => onDone(null)}>Cancel</Button>
          <Button variant="primary" disabled={!text.trim()} onClick={() => onDone(text.trim())}>
            {confirm}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (text.trim()) onDone(text.trim());
        }}
      >
        <label className="field">
          <span className="field-label">{label}</span>
          <input
            ref={(el) => {
              input.current = el;
              if (el && document.activeElement !== el) {
                el.focus();
                if (select === "all") el.select();
              }
            }}
            className="input"
            value={text}
            maxLength={120}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
      </form>
    </Modal>
  );
}

function TopicPicker({
  title,
  tree,
  exclude,
  current,
  confirm,
  onDone,
}: {
  title: string;
  tree: TreeNode[];
  exclude?: string;
  current?: string;
  confirm: string;
  onDone: (v: string | null) => void;
}) {
  const options = topics(tree).filter(({ node }) => !exclude || (node.path !== exclude && !node.path.startsWith(exclude + "/")));
  const [chosen, setChosen] = useState<string>("");
  return (
    <Modal
      open
      title={title}
      width={420}
      onClose={() => onDone(null)}
      footer={
        <>
          <Button onClick={() => onDone(null)}>Cancel</Button>
          <Button variant="primary" onClick={() => onDone(chosen)}>
            {confirm}
          </Button>
        </>
      }
    >
      <div className="picker" role="listbox" aria-label="Topics">
        <button type="button" role="option" aria-selected={chosen === ""} className={`picker-row ${chosen === "" ? "is-selected" : ""}`} onClick={() => setChosen("")}>
          <FolderRoot size={15} strokeWidth={1.75} />
          <span>Top level</span>
          {current === "" && <span className="picker-note">current</span>}
        </button>
        {options.map(({ node, depth }) => (
          <button
            key={node.path}
            type="button"
            role="option"
            aria-selected={chosen === node.path}
            className={`picker-row ${chosen === node.path ? "is-selected" : ""}`}
            style={{ paddingLeft: 12 + (depth + 1) * 16 }}
            onClick={() => setChosen(node.path)}
            onDoubleClick={() => onDone(node.path)}
          >
            <Folder size={15} strokeWidth={1.75} />
            <span>{node.name}</span>
            {node.path === current && <span className="picker-note">current</span>}
          </button>
        ))}
      </div>
    </Modal>
  );
}
