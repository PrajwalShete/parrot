// Myers diff (the algorithm behind `git diff`), by line and then by word inside changed lines,
// so a one-word edit shows as one highlighted word.

export type Op = "equal" | "insert" | "delete";
export interface Edit<T> {
  op: Op;
  items: T[];
}

/** Shortest edit script from `a` to `b`, grouped into runs. */
export function diff<T>(a: T[], b: T[], eq: (x: T, y: T) => boolean = (x, y) => x === y): Edit<T>[] {
  // Trim the common prefix and suffix first: most edits touch a small part of a note.
  let start = 0;
  while (start < a.length && start < b.length && eq(a[start] as T, b[start] as T)) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && eq(a[endA - 1] as T, b[endB - 1] as T)) {
    endA--;
    endB--;
  }
  const mid = myers(a.slice(start, endA), b.slice(start, endB), eq);
  const out: Edit<T>[] = [];
  const push = (op: Op, items: T[]) => {
    if (items.length === 0) return;
    const last = out[out.length - 1];
    if (last && last.op === op) last.items.push(...items);
    else out.push({ op, items: [...items] });
  };
  push("equal", a.slice(0, start));
  for (const e of mid) push(e.op, e.items);
  push("equal", a.slice(endA));
  return out;
}

function myers<T>(a: T[], b: T[], eq: (x: T, y: T) => boolean): Edit<T>[] {
  const n = a.length;
  const m = b.length;
  if (n === 0) return m ? [{ op: "insert", items: b }] : [];
  if (m === 0) return [{ op: "delete", items: a }];
  const max = n + m;
  const offset = max;
  let v: Int32Array = new Int32Array(2 * max + 2);
  const trace: Int32Array[] = [];
  outer: for (let d = 0; d <= max; d++) {
    trace.push(v.slice());
    for (let k = -d; k <= d; k += 2) {
      let x: number;
      if (k === -d || (k !== d && (v[offset + k - 1] ?? 0) < (v[offset + k + 1] ?? 0))) x = v[offset + k + 1] ?? 0;
      else x = (v[offset + k - 1] ?? 0) + 1;
      let y = x - k;
      while (x < n && y < m && eq(a[x] as T, b[y] as T)) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) {
        trace.push(v.slice());
        break outer;
      }
    }
    if (d === max) break;
  }
  // Walk back through the trace to recover the path.
  const steps: { op: Op; item: T }[] = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 2; d >= 0 && (x > 0 || y > 0); d--) {
    v = trace[d] as Int32Array;
    const k = x - y;
    let prevK: number;
    if (k === -d || (k !== d && (v[offset + k - 1] ?? 0) < (v[offset + k + 1] ?? 0))) prevK = k + 1;
    else prevK = k - 1;
    const prevX = v[offset + prevK] ?? 0;
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      steps.push({ op: "equal", item: a[x - 1] as T });
      x--;
      y--;
    }
    if (d > 0) {
      if (x === prevX) steps.push({ op: "insert", item: b[y - 1] as T });
      else steps.push({ op: "delete", item: a[x - 1] as T });
    }
    x = prevX;
    y = prevY;
  }
  steps.reverse();
  const out: Edit<T>[] = [];
  for (const s of steps) {
    const last = out[out.length - 1];
    if (last && last.op === s.op) last.items.push(s.item);
    else out.push({ op: s.op, items: [s.item] });
  }
  return out;
}

// ---- Lines and words ------------------------------------------------------------------------

/** Words, spaces and punctuation as separate tokens (works for Devanagari too). */
export function tokens(line: string): string[] {
  return line.match(/\s+|[\p{L}\p{N}\p{M}_]+|[^\s\p{L}\p{N}\p{M}_]/gu) ?? [];
}

export interface Piece {
  text: string;
  changed: boolean;
}

export type Row =
  | { kind: "equal"; text: string; a: number; b: number }
  | { kind: "delete"; pieces: Piece[]; a: number }
  | { kind: "insert"; pieces: Piece[]; b: number }
  | { kind: "skip"; count: number; a: number; b: number };

// Whole added or removed lines are shown by their row color; words are marked only inside edits.
const whole = (text: string): Piece[] => [{ text, changed: false }];

/** Word-level pieces for a deleted line and its replacement. */
export function wordPieces(before: string, after: string): { del: Piece[]; ins: Piece[] } {
  const parts = diff(tokens(before), tokens(after));
  const del: Piece[] = [];
  const ins: Piece[] = [];
  for (const p of parts) {
    const text = p.items.join("");
    if (p.op === "equal") {
      del.push({ text, changed: false });
      ins.push({ text, changed: false });
    } else if (p.op === "delete") del.push({ text, changed: true });
    else ins.push({ text, changed: true });
  }
  return { del, ins };
}

/**
 * A unified diff as rows: unchanged lines around changes (`context` on each side), long unchanged
 * runs folded into `skip` rows, and changed line pairs highlighted word by word.
 */
export function lineDiff(before: string, after: string, context = 3): Row[] {
  const a = before.split("\n");
  const b = after.split("\n");
  const edits = diff(a, b);
  const rows: Row[] = [];
  let ia = 0;
  let ib = 0;
  edits.forEach((e, i) => {
    if (e.op === "equal") {
      const n = e.items.length;
      const first = i === 0;
      const last = i === edits.length - 1;
      const keepHead = first ? 0 : Math.min(context, n);
      const keepTail = last ? 0 : Math.min(context, n - keepHead);
      const hidden = n - keepHead - keepTail;
      for (let k = 0; k < keepHead; k++) rows.push({ kind: "equal", text: e.items[k] ?? "", a: ia + k, b: ib + k });
      if (hidden > 0) rows.push({ kind: "skip", count: hidden, a: ia + keepHead, b: ib + keepHead });
      for (let k = n - keepTail; k < n; k++) rows.push({ kind: "equal", text: e.items[k] ?? "", a: ia + k, b: ib + k });
      ia += n;
      ib += n;
      return;
    }
    if (e.op === "delete") {
      const next = edits[i + 1];
      if (next?.op === "insert") return; // handled with the insert, so lines can pair up
      e.items.forEach((t, k) => rows.push({ kind: "delete", pieces: whole(t), a: ia + k }));
      ia += e.items.length;
      return;
    }
    // insert, possibly replacing the delete run before it
    const prev = edits[i - 1];
    const dels = prev?.op === "delete" ? prev.items : [];
    const pairs = Math.min(dels.length, e.items.length);
    const delRows: Row[] = [];
    const insRows: Row[] = [];
    dels.forEach((t, k) => {
      if (k < pairs) {
        const w = wordPieces(t, e.items[k] ?? "");
        delRows.push({ kind: "delete", pieces: w.del, a: ia + k });
        insRows.push({ kind: "insert", pieces: w.ins, b: ib + k });
      } else delRows.push({ kind: "delete", pieces: whole(t), a: ia + k });
    });
    e.items.forEach((t, k) => {
      if (k >= pairs) insRows.push({ kind: "insert", pieces: whole(t), b: ib + k });
    });
    rows.push(...delRows, ...insRows);
    ia += dels.length;
    ib += e.items.length;
  });
  return rows;
}

/** Lines added and removed. */
export function stats(rows: Row[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const r of rows) {
    if (r.kind === "insert") added++;
    else if (r.kind === "delete") removed++;
  }
  return { added, removed };
}
