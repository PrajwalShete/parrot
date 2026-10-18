// Typed client for Parrot's local API.

export type Kind = "topic" | "note";

export interface TreeNode {
  name: string;
  path: string;
  kind: Kind;
  modified: number;
  size?: number;
  children?: TreeNode[];
}

export interface Doc {
  path: string;
  name: string;
  content: string;
  modified: number;
}

export interface AppState {
  version: string;
  data_dir: string;
  data_dir_display: string;
  has_key: boolean;
  masked_key: string;
  voice: string;
  voices: string[];
  language_codes: string[];
  models: { transcribe: string; voice: string };
  keys_url: string;
}

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function request<T>(method: string, url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = null;
  }
  if (!res.ok) {
    const message =
      data && typeof data === "object" && "error" in data ? String((data as { error: unknown }).error) : text || res.statusText;
    throw new ApiError(message, res.status);
  }
  return data as T;
}

const q = (path: string) => encodeURIComponent(path);

export const api = {
  state: () => request<AppState>("GET", "/api/state"),
  settings: (s: Partial<{ api_key: string; voice: string; language_codes: string[] }>) =>
    request<AppState>("PUT", "/api/settings", s),
  tree: () => request<{ tree: TreeNode[] }>("GET", "/api/tree").then((r) => r.tree),
  read: (path: string) => request<Doc>("GET", `/api/note?path=${q(path)}`),
  write: (path: string, content: string, expected_modified?: number) =>
    request<Doc>("PUT", "/api/note", { path, content, expected_modified }),
  createNote: (parent: string, title: string, kind: "note" | "transcript" = "note") =>
    request<{ path: string }>("POST", "/api/notes", { parent, title, kind }).then((r) => r.path),
  createTopic: (parent: string, name: string) =>
    request<{ path: string }>("POST", "/api/topics", { parent, name }).then((r) => r.path),
  rename: (path: string, name: string) => request<{ path: string }>("POST", "/api/rename", { path, name }).then((r) => r.path),
  move: (path: string, to: string) => request<{ path: string }>("POST", "/api/move", { path, to }).then((r) => r.path),
  remove: (path: string) => request<{ id: string; path: string }>("POST", "/api/delete", { path }),
  restore: (id: string) => request<{ path: string }>("POST", "/api/restore", { id }).then((r) => r.path),
  reveal: (path = "") => request<{ ok: boolean }>("POST", "/api/reveal", { path }),
};

/// A WebSocket URL on this origin.
export function socketUrl(path: string): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}${path}`;
}
