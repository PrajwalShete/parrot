import { describe, expect, it } from "vitest";
import { diff, lineDiff, stats, tokens, wordPieces } from "./diff";

const apply = (_a: string[], edits: ReturnType<typeof diff<string>>) => edits.filter((e) => e.op !== "delete").flatMap((e) => e.items);
const restore = (edits: ReturnType<typeof diff<string>>) => edits.filter((e) => e.op !== "insert").flatMap((e) => e.items);

describe("diff", () => {
  it("rebuilds both sides from the edit script", () => {
    const cases: [string, string][] = [
      ["abcabba", "cbabac"],
      ["", "abc"],
      ["abc", ""],
      ["same", "same"],
      ["kitten", "sitting"],
    ];
    for (const [x, y] of cases) {
      const a = [...x];
      const b = [...y];
      const e = diff(a, b);
      expect(apply(a, e).join("")).toBe(y);
      expect(restore(e).join("")).toBe(x);
    }
  });

  it("finds a minimal script", () => {
    const e = diff([..."abcabba"], [..."cbabac"]);
    const changes = e.filter((x) => x.op !== "equal").reduce((n, x) => n + x.items.length, 0);
    expect(changes).toBe(5);
  });
});

describe("words", () => {
  it("splits words, spaces and punctuation, including Devanagari", () => {
    expect(tokens("Pune sales, 4.5 lakh")).toEqual(["Pune", " ", "sales", ",", " ", "4", ".", "5", " ", "lakh"]);
    expect(tokens("पुणे में सेल")).toEqual(["पुणे", " ", "में", " ", "सेल"]);
  });

  it("highlights only the word that changed", () => {
    const { del, ins } = wordPieces("Nashik supplier is delayed.", "Nashik supplier is late.");
    expect(del.filter((p) => p.changed).map((p) => p.text)).toEqual(["delayed"]);
    expect(ins.filter((p) => p.changed).map((p) => p.text)).toEqual(["late"]);
  });
});

describe("lineDiff", () => {
  it("pairs a changed line and folds unchanged lines", () => {
    const before = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
    const after = before.replace("line 10", "line ten");
    const rows = lineDiff(before, after, 2);
    expect(stats(rows)).toEqual({ added: 1, removed: 1 });
    expect(rows.filter((r) => r.kind === "skip").map((r) => (r.kind === "skip" ? r.count : 0))).toEqual([8, 7]);
    const del = rows.find((r) => r.kind === "delete");
    expect(del && del.kind === "delete" ? del.pieces.filter((p) => p.changed).map((p) => p.text) : []).toEqual(["10"]);
  });

  it("shows pure additions, like a recording or an appended summary", () => {
    const rows = lineDiff("a\nb", "a\nb\n## Summary\n- one");
    expect(stats(rows)).toEqual({ added: 2, removed: 0 });
  });
});
