import type { TreeNode } from "./api";

export const basename = (path: string) => path.split("/").pop() ?? path;

/** The topic holding `path` ("" for the top level). */
export const parentOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");

/** "Sales/Pune/Standup.md" → ["Sales", "Sales/Pune"] */
export function ancestors(path: string): string[] {
  const parts = path.split("/").slice(0, -1);
  return parts.map((_, i) => parts.slice(0, i + 1).join("/"));
}

export function find(nodes: TreeNode[], path: string): TreeNode | undefined {
  for (const n of nodes) {
    if (n.path === path) return n;
    if (n.children && path.startsWith(n.path + "/")) {
      const hit = find(n.children, path);
      if (hit) return hit;
    }
  }
  return undefined;
}

export function flatten(nodes: TreeNode[], out: TreeNode[] = []): TreeNode[] {
  for (const n of nodes) {
    out.push(n);
    if (n.children) flatten(n.children, out);
  }
  return out;
}

/** Every topic, depth-first, with its depth (for pickers). */
export function topics(nodes: TreeNode[], depth = 0, out: { node: TreeNode; depth: number }[] = []) {
  for (const n of nodes) {
    if (n.kind !== "topic") continue;
    out.push({ node: n, depth });
    topics(n.children ?? [], depth + 1, out);
  }
  return out;
}

/** Notes, newest first. */
export function recentNotes(nodes: TreeNode[], limit = 8): TreeNode[] {
  return flatten(nodes)
    .filter((n) => n.kind === "note")
    .sort((a, b) => b.modified - a.modified)
    .slice(0, limit);
}

/** Where a path ends up after its ancestor moved from `from` to `to`. */
export function rebase(path: string, from: string, to: string): string {
  if (path === from) return to;
  if (path.startsWith(from + "/")) return to + path.slice(from.length);
  return path;
}

/** Counts notes inside a topic, recursively. */
export function noteCount(node: TreeNode): number {
  if (node.kind === "note") return 1;
  return (node.children ?? []).reduce((sum, c) => sum + noteCount(c), 0);
}
