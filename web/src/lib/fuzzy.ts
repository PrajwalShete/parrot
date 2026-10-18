/**
 * Scores how well `query` matches `text` as an in-order subsequence (case-insensitive).
 * Higher is better; null means no match. Rewards consecutive runs and word starts, so
 * "pun std" finds "Pune / Monday standup".
 */
export function score(query: string, text: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return 0;
  const t = text.toLowerCase();
  let ti = 0;
  let total = 0;
  let run = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found < 0) return null;
    const wordStart = found === 0 || /[\s/_\-.]/.test(t[found - 1] ?? "");
    run = found === ti ? run + 1 : 0;
    total += 1 + run * 2 + (wordStart ? 3 : 0) - Math.min(3, (found - ti) * 0.1);
    ti = found + 1;
  }
  // Prefer shorter texts (a tighter match).
  return total - t.length * 0.01;
}

export function rank<T>(query: string, items: T[], key: (item: T) => string, limit = 50): T[] {
  return items
    .map((item) => ({ item, s: score(query, key(item)) }))
    .filter((x): x is { item: T; s: number } => x.s !== null)
    .sort((a, b) => b.s - a.s)
    .slice(0, limit)
    .map((x) => x.item);
}
