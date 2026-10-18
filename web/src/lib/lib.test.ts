import { describe, expect, it } from "vitest";
import { Downsampler, int16ToFloat32, meter, rms } from "../audio/resample";
import type { TreeNode } from "./api";
import { clock, meetingTitle, relativeTime } from "./format";
import { rank, score } from "./fuzzy";
import { blocks, countLines, parseFrontmatter, render, spanSeconds } from "./markdown";
import { ancestors, find, noteCount, parentOf, rebase, recentNotes, topics } from "./tree";
import { addFragment, joinFragment, type Item } from "./useAssistant";

describe("Downsampler", () => {
  it("turns 48 kHz into 16 kHz, one third the samples, across block boundaries", () => {
    const d = new Downsampler(48000, 16000);
    let total = 0;
    for (let i = 0; i < 100; i++) total += d.process(new Float32Array(128)).length;
    expect(Math.abs(total - (12800 * 16) / 48)).toBeLessThanOrEqual(1);
  });

  it("keeps a low tone and its level", () => {
    const d = new Downsampler(48000, 16000);
    const input = new Float32Array(4800).map((_, i) => 0.5 * Math.sin((2 * Math.PI * 300 * i) / 48000));
    const out = int16ToFloat32(d.process(input));
    const peak = Math.max(...out.slice(200).map(Math.abs));
    expect(peak).toBeGreaterThan(0.45);
    expect(peak).toBeLessThan(0.52);
  });

  it("clamps instead of wrapping", () => {
    const d = new Downsampler(16000, 16000);
    const out = d.process(new Float32Array([2, -2, 2, -2]));
    expect(Math.max(...out)).toBe(32767);
    expect(Math.min(...out)).toBe(-32767);
  });

  it("meters silence as zero and loud speech near one", () => {
    expect(rms(new Float32Array(10))).toBe(0);
    expect(meter(0)).toBe(0);
    expect(meter(0.5)).toBeGreaterThan(0.9);
  });
});

describe("markdown", () => {
  const note = "---\ntype: transcript\ncreated: 2026-10-18T10:00:00+05:30\n---\n\n[10:00:04] Okay, let's start.\n[10:02:11] पुणे में 4.5 लाख।\n\n## Summary\n- one\n";

  it("splits front matter", () => {
    const p = parseFrontmatter(note);
    expect(p.meta).toEqual({ type: "transcript", created: "2026-10-18T10:00:00+05:30" });
    expect(p.body.startsWith("\n[10:00:04]")).toBe(true);
    expect(parseFrontmatter("no front matter").body).toBe("no front matter");
  });

  it("turns transcript lines into rows and keeps the rest as markdown", () => {
    const b = blocks(parseFrontmatter(note).body);
    expect(b).toEqual([
      { kind: "line", time: "10:00:04", text: "Okay, let's start." },
      { kind: "line", time: "10:02:11", text: "पुणे में 4.5 लाख।" },
      { kind: "md", text: "## Summary\n- one" },
    ]);
    expect(countLines(note)).toBe(2);
    expect(spanSeconds(note)).toBe(127);
  });

  it("sanitizes html", () => {
    const html = render("hi <img src=x onerror=alert(1)> <script>alert(1)</script>");
    expect(html).not.toContain("onerror");
    expect(html).not.toContain("<script");
  });
});

describe("fuzzy", () => {
  it("matches subsequences and prefers word starts", () => {
    expect(score("pst", "Pune standup")).not.toBeNull();
    expect(score("xyz", "Pune standup")).toBeNull();
    const ranked = rank("mon", ["Common notes", "Monday standup"], (s) => s);
    expect(ranked[0]).toBe("Monday standup");
  });
});

const tree: TreeNode[] = [
  {
    name: "Sales",
    path: "Sales",
    kind: "topic",
    modified: 1,
    children: [
      { name: "Pune", path: "Sales/Pune", kind: "topic", modified: 1, children: [{ name: "Standup", path: "Sales/Pune/Standup.md", kind: "note", modified: 30 }] },
      { name: "Plan", path: "Sales/Plan.md", kind: "note", modified: 20 },
    ],
  },
  { name: "Welcome", path: "Welcome.md", kind: "note", modified: 10 },
];

describe("tree", () => {
  it("navigates paths", () => {
    expect(parentOf("Sales/Pune/Standup.md")).toBe("Sales/Pune");
    expect(parentOf("Welcome.md")).toBe("");
    expect(ancestors("Sales/Pune/Standup.md")).toEqual(["Sales", "Sales/Pune"]);
    expect(find(tree, "Sales/Pune/Standup.md")?.name).toBe("Standup");
    expect(find(tree, "Nope")).toBeUndefined();
  });

  it("lists topics and recent notes", () => {
    expect(topics(tree).map((t) => [t.node.path, t.depth])).toEqual([
      ["Sales", 0],
      ["Sales/Pune", 1],
    ]);
    expect(recentNotes(tree).map((n) => n.name)).toEqual(["Standup", "Plan", "Welcome"]);
    expect(noteCount(tree[0]!)).toBe(2);
  });

  it("rebases paths after a move", () => {
    expect(rebase("Sales/Pune/Standup.md", "Sales/Pune", "Pune")).toBe("Pune/Standup.md");
    expect(rebase("Sales/Pune", "Sales/Pune", "Pune")).toBe("Pune");
    expect(rebase("Sales/Punekar", "Sales/Pune", "Pune")).toBe("Sales/Punekar");
  });
});

describe("format", () => {
  it("formats clocks and times", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(754)).toBe("12:34");
    expect(clock(3754)).toBe("1:02:34");
    const now = Date.now();
    expect(relativeTime(now - 5000, now)).toBe("just now");
    expect(meetingTitle(new Date(2026, 9, 18, 11, 2))).not.toContain(":");
  });
});

describe("assistant transcript", () => {
  it("joins streamed fragments", () => {
    expect(joinFragment("", " Hello")).toBe("Hello");
    expect(joinFragment("Hello", "world")).toBe("Hello world");
    expect(joinFragment("Hello", ", world")).toBe("Hello, world");
    expect(joinFragment("Hello ", "world")).toBe("Hello world");
  });

  it("extends the open message and starts a new one per speaker", () => {
    let items: Item[] = [];
    items = addFragment(items, "user", "What were");
    items = addFragment(items, "user", "the sales?");
    items = addFragment(items, "agent", "4.5 lakh.");
    expect(items.map((i) => (i.role === "tool" ? i.name : [i.role, i.text, i.done]))).toEqual([
      ["user", "What were the sales?", true],
      ["agent", "4.5 lakh.", false],
    ]);
  });
});
