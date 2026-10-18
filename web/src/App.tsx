import { AudioLines, ChevronRight, FilePlus2, FileText, FolderInput, FolderOpen, FolderPlus, Mic, MoreHorizontal, PanelLeftOpen, Pencil, Settings, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AssistantPanel } from "./components/AssistantPanel";
import { type Command, CommandPalette } from "./components/CommandPalette";
import { useFeedback } from "./components/feedback";
import { NoteView } from "./components/NoteView";
import { Home, TopicView } from "./components/Pages";
import { Button, IconButton, Menu, type MenuEntry, mod } from "./components/primitives";
import { RecordDock } from "./components/RecordDock";
import { SettingsDialog, type Theme } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { api, type AppState, type TreeNode } from "./lib/api";
import { meetingTitle } from "./lib/format";
import { ancestors, find, parentOf, rebase } from "./lib/tree";
import { useAssistant } from "./lib/useAssistant";
import { useRecorder } from "./lib/useRecorder";

type Route = { kind: "home" } | { kind: "note"; path: string } | { kind: "topic"; path: string };

function parseHash(hash: string): Route {
  const m = /^#\/(note|topic)\/(.+)$/.exec(hash);
  if (!m) return { kind: "home" };
  return { kind: m[1] as "note" | "topic", path: decodeURIComponent(m[2] ?? "") };
}

function hashFor(route: Route): string {
  return route.kind === "home" ? "#/" : `#/${route.kind}/${encodeURIComponent(route.path)}`;
}

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(`parrot.${key}`);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function persist(key: string, value: unknown) {
  try {
    localStorage.setItem(`parrot.${key}`, JSON.stringify(value));
  } catch {
    // Private windows can refuse storage; preferences just don't stick.
  }
}

const NARROW = 860;
const MEDIUM = 1180;

const isTyping = (el: EventTarget | null) => el instanceof HTMLElement && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));

export default function App() {
  const fb = useFeedback();
  const [state, setState] = useState<AppState | null>(null);
  const [tree, setTree] = useState<TreeNode[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [offline, setOffline] = useState(false);
  const [route, setRoute] = useState<Route>(() => parseHash(location.hash));
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(load<string[]>("expanded", [])));
  const [versions, setVersions] = useState<Record<string, number>>({});
  // On narrow windows these are overlays: start closed, and never show both at once.
  const [sidebar, setSidebarState] = useState(() => window.innerWidth > NARROW && load("sidebar", true));
  const [assistantOpen, setAssistantState] = useState(() => window.innerWidth > MEDIUM && load("assistant", true));
  const setSidebar = useCallback((v: boolean | ((s: boolean) => boolean)) => {
    setSidebarState((s) => {
      const next = typeof v === "function" ? v(s) : v;
      if (next && window.innerWidth <= MEDIUM) setAssistantState(false);
      return next;
    });
  }, []);
  const setAssistantOpen = useCallback((v: boolean | ((s: boolean) => boolean)) => {
    setAssistantState((s) => {
      const next = typeof v === "function" ? v(s) : v;
      if (next && window.innerWidth <= NARROW) setSidebarState(false);
      return next;
    });
  }, []);
  const [palette, setPalette] = useState(false);
  const [settings, setSettings] = useState<{ open: boolean; focusKey: boolean }>({ open: false, focusKey: false });
  const [editing, setEditing] = useState(false);
  const [menu, setMenu] = useState<{ at: { x: number; y: number }; items: MenuEntry[] } | null>(null);
  const [theme, setTheme] = useState<Theme>(() => load("theme", "system"));
  const recorder = useRecorder();

  // ---- Data ------------------------------------------------------------------------------------

  const refreshTree = useCallback(async () => {
    try {
      setTree(await api.tree());
      setLoaded(true);
      setOffline(false);
    } catch {
      setOffline(true);
    }
  }, []);

  useEffect(() => {
    void refreshTree();
    api.state().then(setState, () => setOffline(true));
  }, [refreshTree]);

  // Live updates: whatever changes the folder (a recording, the assistant, another editor).
  useEffect(() => {
    let timer = 0;
    const es = new EventSource("/api/events");
    es.onmessage = (e) => {
      const msg = JSON.parse(e.data as string) as { type: string; paths?: string[] };
      if (msg.type !== "changed") return;
      const paths = msg.paths ?? [];
      setVersions((v) => {
        const next = { ...v };
        for (const p of paths) next[p] = (next[p] ?? 0) + 1;
        if (paths.includes("")) next["*"] = (next["*"] ?? 0) + 1;
        return next;
      });
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void refreshTree(), 80);
    };
    es.onopen = () => {
      setOffline(false);
      void refreshTree();
    };
    es.onerror = () => setOffline(true);
    return () => {
      window.clearTimeout(timer);
      es.close();
    };
  }, [refreshTree]);

  // ---- Routing ---------------------------------------------------------------------------------

  useEffect(() => {
    const onHash = () => setRoute(parseHash(location.hash));
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const go = useCallback((r: Route) => {
    setEditing(false);
    if (location.hash !== hashFor(r)) location.hash = hashFor(r);
    setRoute(r);
    if (window.innerWidth <= NARROW) setSidebar(false);
  }, [setSidebar]);

  const toggle = useCallback((path: string, open?: boolean) => {
    setExpanded((s) => {
      const next = new Set(s);
      const willOpen = open ?? !s.has(path);
      if (willOpen) next.add(path);
      else next.delete(path);
      persist("expanded", [...next]);
      return next;
    });
  }, []);

  const reveal = useCallback(
    (path: string) => {
      const parents = ancestors(path);
      if (parents.every((p) => expanded.has(p))) return;
      setExpanded((s) => {
        const next = new Set([...s, ...parents]);
        persist("expanded", [...next]);
        return next;
      });
    },
    [expanded],
  );

  useEffect(() => {
    if (route.kind !== "home") reveal(route.path);
  }, [route, reveal]);

  // Remember the layout people choose on wide screens only.
  useEffect(() => {
    if (window.innerWidth > NARROW) persist("sidebar", sidebar);
  }, [sidebar]);
  useEffect(() => {
    if (window.innerWidth > MEDIUM) persist("assistant", assistantOpen);
  }, [assistantOpen]);
  useEffect(() => {
    persist("theme", theme);
    if (theme === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = theme;
  }, [theme]);

  const current = route.kind === "home" ? undefined : find(tree, route.path);
  const currentTopic = route.kind === "topic" ? route.path : route.kind === "note" ? parentOf(route.path) : "";
  useEffect(() => {
    const name = current?.name;
    document.title = recorder.state !== "idle" ? `● Recording · Parrot` : name ? `${name} · Parrot` : "Parrot";
  }, [current?.name, recorder.state]);

  // ---- Assistant ------------------------------------------------------------------------------

  const view = useMemo(() => ({ note: route.kind === "note" ? route.path : null, topic: route.kind === "home" ? null : currentTopic }), [route, currentTopic]);
  const onAgentChanged = useCallback(() => void refreshTree(), [refreshTree]);
  const assistant = useAssistant(view, onAgentChanged);

  // ---- Actions --------------------------------------------------------------------------------

  const fail = useCallback((e: unknown) => fb.toast(e instanceof Error ? e.message : String(e), { tone: "error" }), [fb]);
  const hasKey = !!state?.has_key;
  const needKey = useCallback(() => setSettings({ open: true, focusKey: true }), []);

  const open = useCallback((n: TreeNode) => go({ kind: n.kind, path: n.path }), [go]);

  const newTopic = useCallback(
    async (parent = "") => {
      const name = await fb.prompt({ title: parent ? "New subtopic" : "New topic", label: parent ? `Inside ${parent.replaceAll("/", " / ")}` : "Name", confirm: "Create" });
      if (!name) return;
      try {
        const path = await api.createTopic(parent, name);
        await refreshTree();
        if (parent) toggle(parent, true);
        go({ kind: "topic", path });
      } catch (e) {
        fail(e);
      }
    },
    [fb, refreshTree, toggle, go, fail],
  );

  const newNote = useCallback(
    async (parent = "") => {
      const title = await fb.prompt({ title: "New note", label: parent ? `In ${parent.replaceAll("/", " / ")}` : "Title", confirm: "Create" });
      if (!title) return;
      try {
        const path = await api.createNote(parent, title, "note");
        await refreshTree();
        go({ kind: "note", path });
        setEditing(true);
      } catch (e) {
        fail(e);
      }
    },
    [fb, refreshTree, go, fail],
  );

  const recordInto = useCallback(
    async (path: string) => {
      if (!hasKey) return needKey();
      if (recorder.state !== "idle") return fb.toast("Already recording. Stop it first.");
      go({ kind: "note", path });
      await recorder.start(path);
    },
    [hasKey, needKey, recorder, fb, go],
  );

  const record = useCallback(
    async (topic?: string) => {
      if (!hasKey) return needKey();
      if (recorder.state !== "idle") {
        if (recorder.path) go({ kind: "note", path: recorder.path });
        return;
      }
      const where = topic ?? currentTopic;
      const title = await fb.prompt({
        title: "New recording",
        label: where ? `Saved in ${where.replaceAll("/", " / ")}` : "Saved at the top level",
        value: meetingTitle(),
        confirm: "Start recording",
      });
      if (!title) return;
      try {
        const path = await api.createNote(where, title, "transcript");
        await refreshTree();
        await recordInto(path);
      } catch (e) {
        fail(e);
      }
    },
    [hasKey, needKey, recorder, currentTopic, fb, refreshTree, recordInto, go, fail],
  );

  /** After something moved or was renamed, keep the open page pointing at it. */
  const follow = useCallback(
    (from: string, to: string) => {
      if (route.kind !== "home" && (route.path === from || route.path.startsWith(from + "/"))) go({ kind: route.kind, path: rebase(route.path, from, to) });
      setExpanded((s) => {
        const next = new Set([...s].map((p) => rebase(p, from, to)));
        persist("expanded", [...next]);
        return next;
      });
    },
    [route, go],
  );

  const renamePath = useCallback(
    async (path: string, name: string) => {
      try {
        const to = await api.rename(path, name);
        await refreshTree();
        follow(path, to);
      } catch (e) {
        fail(e);
        throw e;
      }
    },
    [refreshTree, follow, fail],
  );

  const rename = useCallback(
    async (node: TreeNode) => {
      const name = await fb.prompt({ title: `Rename ${node.kind}`, label: "Name", value: node.name, confirm: "Rename" });
      if (name && name !== node.name) await renamePath(node.path, name).catch(() => {});
    },
    [fb, renamePath],
  );

  const moveTo = useCallback(
    async (from: string, to: string) => {
      try {
        const dest = await api.move(from, to);
        await refreshTree();
        if (to) toggle(to, true);
        follow(from, dest);
        fb.toast(`Moved to ${to ? to.replaceAll("/", " / ") : "the top level"}`);
      } catch (e) {
        fail(e);
      }
    },
    [refreshTree, toggle, follow, fb, fail],
  );

  const move = useCallback(
    async (node: TreeNode) => {
      const to = await fb.pickTopic({ title: `Move “${node.name}”`, tree, exclude: node.kind === "topic" ? node.path : undefined, current: parentOf(node.path), confirm: "Move here" });
      if (to !== null && to !== parentOf(node.path)) await moveTo(node.path, to);
    },
    [fb, tree, moveTo],
  );

  const remove = useCallback(
    async (node: TreeNode) => {
      if (recorder.path && (recorder.path === node.path || recorder.path.startsWith(node.path + "/"))) {
        return fb.toast("That's being recorded into. Stop the recording first.");
      }
      const ok = await fb.confirm({
        title: `Delete ${node.kind === "topic" ? "topic" : "note"}?`,
        body: node.kind === "topic" ? <>“{node.name}” and everything inside it will be moved to the trash.</> : <>“{node.name}” will be moved to the trash.</>,
        confirm: "Delete",
        danger: true,
      });
      if (!ok) return;
      try {
        const trashed = await api.remove(node.path);
        await refreshTree();
        if (route.kind !== "home" && (route.path === node.path || route.path.startsWith(node.path + "/"))) {
          const up = parentOf(node.path);
          go(up ? { kind: "topic", path: up } : { kind: "home" });
        }
        fb.toast(`Deleted “${node.name}”`, {
          action: {
            label: "Undo",
            run: () =>
              void api
                .restore(trashed.id)
                .then(() => refreshTree())
                .catch(fail),
          },
        });
      } catch (e) {
        fail(e);
      }
    },
    [recorder.path, fb, refreshTree, route, go, fail],
  );

  const revealIn = useCallback((path = "") => void api.reveal(path).catch(fail), [fail]);

  const nodeMenu = useCallback(
    (node: TreeNode): MenuEntry[] => {
      const common: MenuEntry[] = [
        { label: "Rename", icon: <Pencil size={14} />, onSelect: () => void rename(node), shortcut: "F2" },
        { label: "Move to…", icon: <FolderInput size={14} />, onSelect: () => void move(node) },
        { label: "Show in folder", icon: <FolderOpen size={14} />, onSelect: () => revealIn(node.path) },
        "separator",
        { label: "Delete", icon: <Trash2 size={14} />, danger: true, onSelect: () => void remove(node) },
      ];
      if (node.kind === "topic") {
        return [
          { label: "Record here", icon: <Mic size={14} />, onSelect: () => void record(node.path) },
          { label: "New note", icon: <FilePlus2 size={14} />, onSelect: () => void newNote(node.path) },
          { label: "New subtopic", icon: <FolderPlus size={14} />, onSelect: () => void newTopic(node.path) },
          "separator",
          ...common,
        ];
      }
      return [
        { label: "Open", icon: <FileText size={14} />, onSelect: () => open(node) },
        { label: "Record into this note", icon: <Mic size={14} />, onSelect: () => void recordInto(node.path), disabled: recorder.state !== "idle" },
        "separator",
        ...common,
      ];
    },
    [rename, move, revealIn, remove, record, newNote, newTopic, open, recordInto, recorder.state],
  );

  const treeActions = useMemo(
    () => ({
      open,
      menu: (node: TreeNode, at: { x: number; y: number }) => setMenu({ at, items: nodeMenu(node) }),
      addMenu: (topic: TreeNode, at: { x: number; y: number }) =>
        setMenu({
          at,
          items: [
            { label: "New recording", icon: <Mic size={14} />, onSelect: () => void record(topic.path) },
            { label: "New note", icon: <FilePlus2 size={14} />, onSelect: () => void newNote(topic.path) },
            { label: "New subtopic", icon: <FolderPlus size={14} />, onSelect: () => void newTopic(topic.path) },
          ],
        }),
      rename: (n: TreeNode) => void rename(n),
      remove: (n: TreeNode) => void remove(n),
      drop: (from: string, to: string) => void moveTo(from, to),
    }),
    [open, nodeMenu, record, newNote, newTopic, rename, remove, moveTo],
  );

  // ---- Keyboard -------------------------------------------------------------------------------

  const keys = useRef({ route, editing, current });
  keys.current = { route, editing, current };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      } else if (cmd && e.key === "\\") {
        e.preventDefault();
        setSidebar((s) => !s);
      } else if (cmd && e.key.toLowerCase() === "j") {
        e.preventDefault();
        setAssistantOpen((a) => !a);
      } else if (!cmd && !e.altKey && !isTyping(e.target) && document.querySelector("dialog[open]") === null) {
        const { route: r, editing: ed } = keys.current;
        if (e.key === "e" && r.kind === "note" && !ed) {
          e.preventDefault();
          setEditing(true);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setSidebar, setAssistantOpen]);

  const commands = useMemo<Command[]>(
    () => [
      { id: "record", label: recorder.state === "idle" ? "New recording" : "Go to the recording", icon: <Mic size={15} />, hint: currentTopic ? `in ${currentTopic.replaceAll("/", " / ")}` : undefined, run: () => void record() },
      { id: "note", label: "New note", icon: <FilePlus2 size={15} />, run: () => void newNote(currentTopic) },
      { id: "topic", label: "New topic", icon: <FolderPlus size={15} />, run: () => void newTopic() },
      { id: "assistant", label: assistantOpen ? "Hide assistant" : "Talk to the assistant", icon: <AudioLines size={15} />, hint: `${mod}J`, run: () => setAssistantOpen((a) => !a) },
      { id: "settings", label: "Settings", icon: <Settings size={15} />, run: () => setSettings({ open: true, focusKey: false }) },
      { id: "folder", label: "Show notebook folder", icon: <FolderOpen size={15} />, run: () => revealIn("") },
    ],
    [recorder.state, currentTopic, record, newNote, newTopic, assistantOpen, setAssistantOpen, revealIn],
  );

  // ---- Render ---------------------------------------------------------------------------------

  const crumbs = route.kind === "home" ? [] : route.path.split("/");
  const noteVersion = route.kind === "note" ? (versions[route.path] ?? 0) + (versions["*"] ?? 0) : 0;
  const showingRecording = route.kind === "note" && route.path === recorder.path;

  return (
    <div className={`app ${sidebar ? "has-sidebar" : ""} ${assistantOpen ? "has-assistant" : ""}`}>
      {sidebar && (
        <>
          <Sidebar
            tree={tree}
            loaded={loaded}
            selected={route.kind === "home" ? null : route.path}
            expanded={expanded}
            onToggle={toggle}
            recording={recorder.state !== "idle" ? recorder.path : null}
            actions={treeActions}
            hasKey={hasKey || !state}
            folder={state?.data_dir_display ?? "Notebook"}
            onSearch={() => setPalette(true)}
            onRecord={() => void record()}
            onNewTopic={() => void newTopic()}
            onSettings={() => setSettings({ open: true, focusKey: false })}
            onReveal={() => revealIn("")}
            onCollapse={() => setSidebar(false)}
          />
          <div className="scrim scrim-left" onClick={() => setSidebar(false)} aria-hidden />
        </>
      )}

      <main className="main">
        <header className="topbar">
          <div className="topbar-left">
            {!sidebar && (
              <IconButton label={`Show sidebar (${mod}\\)`} onClick={() => setSidebar(true)}>
                <PanelLeftOpen size={16} strokeWidth={1.75} />
              </IconButton>
            )}
            <nav className="crumbs" aria-label="Breadcrumb">
              <a href="#/" className="crumb">
                Notebook
              </a>
              {crumbs.map((part, i) => {
                const path = crumbs.slice(0, i + 1).join("/");
                const last = i === crumbs.length - 1;
                return (
                  <span key={path} className="crumb-wrap">
                    <ChevronRight size={13} strokeWidth={2} className="crumb-sep" aria-hidden />
                    {last ? (
                      <span className="crumb is-current" aria-current="page">
                        {part.replace(/\.md$/, "")}
                      </span>
                    ) : (
                      <a className="crumb" href={hashFor({ kind: "topic", path })}>
                        {part}
                      </a>
                    )}
                  </span>
                );
              })}
            </nav>
          </div>
          <div className="topbar-right">
            {offline && <span className="offline">Parrot isn't running · start it with `parrot`</span>}
            {route.kind === "note" && current && (
              <>
                {!showingRecording && (
                  <Button size="sm" variant={editing ? "primary" : "ghost"} onClick={() => setEditing(!editing)}>
                    {editing ? "Done" : "Edit"}
                  </Button>
                )}
                {recorder.state === "idle" && (
                  <Button size="sm" variant="ghost" onClick={() => void recordInto(route.path)}>
                    <Mic size={14} strokeWidth={2} /> Record
                  </Button>
                )}
              </>
            )}
            {current && (
              <IconButton
                label="More"
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  setMenu({ at: { x: r.right - 200, y: r.bottom + 4 }, items: nodeMenu(current) });
                }}
              >
                <MoreHorizontal size={16} strokeWidth={1.75} />
              </IconButton>
            )}
            <IconButton label={`Assistant (${mod}J)`} active={assistantOpen} onClick={() => setAssistantOpen((a) => !a)}>
              <AudioLines size={16} strokeWidth={1.75} />
            </IconButton>
          </div>
        </header>

        <div className="content">
          {route.kind === "note" ? (
            <NoteView
              key={route.path}
              path={route.path}
              version={noteVersion}
              recorder={recorder}
              editing={editing && !showingRecording}
              setEditing={setEditing}
              onRename={renamePath}
              onMissing={() => (loaded ? go({ kind: "home" }) : undefined)}
            />
          ) : route.kind === "topic" && current?.kind === "topic" ? (
            <TopicView topic={current} onOpen={open} onRename={renamePath} onRecord={(t) => void record(t)} onNewNote={(t) => void newNote(t)} onNewTopic={(t) => void newTopic(t)} />
          ) : route.kind === "topic" && loaded ? (
            <div className="doc-state">
              <p>That topic doesn't exist anymore.</p>
              <Button onClick={() => go({ kind: "home" })}>Go home</Button>
            </div>
          ) : (
            <Home tree={tree} loaded={loaded} onOpen={open} onRecord={() => void record("")} onNewTopic={() => void newTopic()} hasKey={hasKey || !state} onSettings={needKey} />
          )}
        </div>

        {recorder.state !== "idle" && <RecordDock recorder={recorder} showing={showingRecording} onShow={() => recorder.path && go({ kind: "note", path: recorder.path })} />}
        {recorder.error && (
          <div className="dock is-error" role="alert">
            <span>{recorder.error}</span>
            <button type="button" className="link" onClick={recorder.clearError}>
              Dismiss
            </button>
          </div>
        )}
      </main>

      {assistantOpen && (
        <>
          <AssistantPanel assistant={assistant} onClose={() => setAssistantOpen(false)} hasKey={hasKey || !state} onSettings={needKey} />
          <div className="scrim scrim-right" onClick={() => setAssistantOpen(false)} aria-hidden />
        </>
      )}

      <CommandPalette open={palette} onClose={() => setPalette(false)} tree={tree} commands={commands} onOpen={open} />
      <SettingsDialog open={settings.open} focusKey={settings.focusKey} onClose={() => setSettings({ open: false, focusKey: false })} state={state} onState={setState} theme={theme} onTheme={setTheme} />
      {menu && <Menu at={menu.at} items={menu.items} onClose={() => setMenu(null)} />}
    </div>
  );
}
