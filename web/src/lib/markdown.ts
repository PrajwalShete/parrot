import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({ gfm: true, breaks: false });

export interface Parsed {
  meta: Record<string, string>;
  body: string;
}

/** Splits YAML-ish front matter (`---\nkey: value\n---`) from the body. */
export function parseFrontmatter(content: string): Parsed {
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(content);
  if (!m) return { meta: {}, body: content };
  const meta: Record<string, string> = {};
  for (const line of (m[1] ?? "").split(/\r?\n/)) {
    const i = line.indexOf(":");
    if (i > 0) meta[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  return { meta, body: content.slice(m[0].length) };
}

export type Block = { kind: "line"; time: string; text: string } | { kind: "md"; text: string };

const LINE = /^\[(\d{1,2}:\d{2}(?::\d{2})?)\]\s?(.*)$/;

/** Transcript lines (`[10:02:15] text`) become timed rows; everything else stays Markdown. */
export function blocks(body: string): Block[] {
  const out: Block[] = [];
  let md: string[] = [];
  const flush = () => {
    const text = md.join("\n").trim();
    if (text) out.push({ kind: "md", text });
    md = [];
  };
  for (const line of body.split(/\r?\n/)) {
    const m = LINE.exec(line);
    if (m) {
      flush();
      out.push({ kind: "line", time: m[1] ?? "", text: m[2] ?? "" });
    } else {
      md.push(line);
    }
  }
  flush();
  return out;
}

export function countLines(body: string): number {
  return body.split(/\r?\n/).filter((l) => LINE.test(l)).length;
}

/** Markdown → safe HTML (notes are local, but the assistant writes them, so sanitize anyway). */
export function render(md: string): string {
  const html = marked.parse(md, { async: false });
  return DOMPurify.sanitize(html, { USE_PROFILES: { html: true } });
}

/** Seconds between the first and last transcript line, or 0. */
export function spanSeconds(body: string): number {
  const times = body
    .split(/\r?\n/)
    .map((l) => LINE.exec(l)?.[1])
    .filter((t): t is string => !!t)
    .map((t) => t.split(":").map(Number))
    .map(([h = 0, m = 0, s = 0]) => h * 3600 + m * 60 + s);
  if (times.length < 2) return 0;
  const span = (times[times.length - 1] ?? 0) - (times[0] ?? 0);
  return span < 0 ? span + 86400 : span;
}
