//! The voice assistant's tools: read, search and edit the notebook, plus a small, safe shell.
//!
//! Everything runs inside the data folder through [`Store`]. Edits at scale go through
//! `replace_in_notes` (what you'd use `sed -i` for) instead of a raw shell, so the assistant can
//! rewrite many notes at once but can't run arbitrary programs or write outside the folder.
//! `run_command` covers the read-only Unix tools (grep, wc, sort, find, ...) with pipes.

use crate::store::{Kind, Node, Store, StoreError};
use anyhow::{Result, anyhow, bail};
use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_OUTPUT: usize = 24_000;
const MAX_READ_LINES: usize = 600;
const MAX_MATCHES: usize = 200;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// What the browser is showing, so "this note" means something.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct View {
    pub note: Option<String>,
    pub topic: Option<String>,
}

/// One tool call's outcome: what the model gets back, a line for the activity feed, and which
/// notes changed (so the UI can refresh).
#[derive(Debug)]
pub struct Outcome {
    pub ok: bool,
    pub response: Value,
    pub summary: String,
    pub changed: Vec<String>,
}

impl Outcome {
    fn ok(summary: impl Into<String>, output: impl Into<Value>) -> Self {
        Self {
            ok: true,
            response: json!({ "output": output.into() }),
            summary: summary.into(),
            changed: Vec::new(),
        }
    }

    fn changed(mut self, paths: impl IntoIterator<Item = String>) -> Self {
        self.changed.extend(paths);
        self
    }
}

/// Function declarations for the Gemini Live setup message.
pub fn declarations() -> Value {
    let s = |d: &str| json!({ "type": "STRING", "description": d });
    let b = |d: &str| json!({ "type": "BOOLEAN", "description": d });
    let n = |d: &str| json!({ "type": "INTEGER", "description": d });
    let obj = |props: Value, required: &[&str]| json!({ "type": "OBJECT", "properties": props, "required": required });
    json!([
        {
            "name": "get_current_view",
            "description": "What the user is looking at right now: the open note and its topic. Call this when the user says 'this note', 'here' or 'this meeting'.",
            "parameters": obj(json!({}), &[])
        },
        {
            "name": "list_notes",
            "description": "Lists topics (folders), subtopics and notes as an indented outline with their paths. Start here to find things.",
            "parameters": obj(json!({ "path": s("Topic to list; omit for everything") }), &[])
        },
        {
            "name": "read_note",
            "description": "Reads a note with line numbers. Long notes: read a range with start_line/end_line.",
            "parameters": obj(json!({
                "path": s("Note path, e.g. 'Sales/Pune/Standup.md'"),
                "start_line": n("First line to read (1-based)"),
                "end_line": n("Last line to read")
            }), &["path"])
        },
        {
            "name": "search_notes",
            "description": "Searches every note (or one topic) for text and returns matching lines as path:line: text. Works across Marathi, Hindi and English.",
            "parameters": obj(json!({
                "query": s("Text or regular expression"),
                "path": s("Limit to this topic or note"),
                "regex": b("Treat query as a regular expression"),
                "case_sensitive": b("Match case exactly (default: no)")
            }), &["query"])
        },
        {
            "name": "create_note",
            "description": "Creates a new Markdown note in a topic.",
            "parameters": obj(json!({
                "topic": s("Topic path; empty string for the top level"),
                "title": s("Note title (becomes the file name)"),
                "content": s("Markdown content")
            }), &["topic", "title", "content"])
        },
        {
            "name": "edit_note",
            "description": "Replaces exact text in one note. 'find' must match exactly once unless replace_all is true. Read the note first.",
            "parameters": obj(json!({
                "path": s("Note path"),
                "find": s("Exact text to replace"),
                "replace": s("New text"),
                "replace_all": b("Replace every occurrence")
            }), &["path", "find", "replace"])
        },
        {
            "name": "append_to_note",
            "description": "Adds text to the end of a note, e.g. a summary or action items.",
            "parameters": obj(json!({ "path": s("Note path"), "text": s("Markdown to append") }), &["path", "text"])
        },
        {
            "name": "write_note",
            "description": "Replaces a note's whole content (or creates the file). Prefer edit_note for small changes.",
            "parameters": obj(json!({ "path": s("Note path ending in .md"), "content": s("Full new content") }), &["path", "content"])
        },
        {
            "name": "replace_in_notes",
            "description": "Find and replace across many notes at once, like sed -i. Use dry_run first to see what would change. Regex replacements can use $1, $2 for groups.",
            "parameters": obj(json!({
                "pattern": s("Text or regular expression to find"),
                "replacement": s("Replacement text"),
                "path": s("Limit to this topic or note; omit for all notes"),
                "regex": b("Treat pattern as a regular expression"),
                "case_sensitive": b("Match case exactly (default: yes)"),
                "dry_run": b("Only report what would change")
            }), &["pattern", "replacement"])
        },
        {
            "name": "create_topic",
            "description": "Creates a topic, or a subtopic inside another topic.",
            "parameters": obj(json!({ "parent": s("Parent topic path; empty for top level"), "name": s("Topic name") }), &["name"])
        },
        {
            "name": "rename_item",
            "description": "Renames a topic or note.",
            "parameters": obj(json!({ "path": s("Current path"), "new_name": s("New name (no folder)") }), &["path", "new_name"])
        },
        {
            "name": "move_item",
            "description": "Moves a topic or note into another topic.",
            "parameters": obj(json!({ "path": s("What to move"), "to_topic": s("Destination topic; empty for top level") }), &["path", "to_topic"])
        },
        {
            "name": "delete_item",
            "description": "Moves a topic or note to the trash (it can be restored). Always confirm with the user before deleting.",
            "parameters": obj(json!({ "path": s("What to delete") }), &["path"])
        },
        {
            "name": "run_command",
            "description": "Runs read-only Unix commands in the notes folder, with pipes: ls, cat, head, tail, wc, grep, sort, uniq, cut, tr, find, diff. Example: grep -ri 'pune' . | wc -l. No redirection; use the edit tools to change files.",
            "parameters": obj(json!({ "command": s("The command line") }), &["command"])
        }
    ])
}

/// Runs one tool call. Errors become `{ "error": ... }` for the model rather than failures.
pub fn execute(store: &Store, view: &View, name: &str, args: &Value) -> Outcome {
    match run(store, view, name, args) {
        Ok(outcome) => outcome,
        Err(e) => {
            let msg = match e.downcast_ref::<StoreError>() {
                Some(se) => se.to_string(),
                None => format!("{e:#}"),
            };
            Outcome {
                ok: false,
                response: json!({ "error": msg }),
                summary: format!("{} failed: {msg}", label(name)),
                changed: Vec::new(),
            }
        }
    }
}

fn label(name: &str) -> &str {
    match name {
        "get_current_view" => "Checking the open note",
        "list_notes" => "Listing notes",
        "read_note" => "Reading",
        "search_notes" => "Searching",
        "create_note" => "Creating a note",
        "edit_note" => "Editing",
        "append_to_note" => "Appending",
        "write_note" => "Writing",
        "replace_in_notes" => "Replacing",
        "create_topic" => "Creating a topic",
        "rename_item" => "Renaming",
        "move_item" => "Moving",
        "delete_item" => "Deleting",
        "run_command" => "Command",
        other => other,
    }
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| anyhow!("missing '{key}'"))
}

fn opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

fn flag(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn num(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
        .map(|n| n as usize)
}

/// Notes the model names without ".md" still resolve.
fn note_path(store: &Store, path: &str) -> String {
    let p = path.trim().trim_start_matches("./").to_string();
    if store.resolve(&p).map(|r| r.is_file()).unwrap_or(false) || p.ends_with(".md") || p.ends_with(".txt") {
        return p;
    }
    format!("{p}.md")
}

fn run(store: &Store, view: &View, name: &str, args: &Value) -> Result<Outcome> {
    Ok(match name {
        "get_current_view" => Outcome::ok("Checked the open note", json!({ "open_note": view.note, "topic": view.topic })),
        "list_notes" => {
            let tree = store.tree()?;
            let scope = opt(args, "path").map(|p| p.trim_matches('/').to_string());
            let nodes: &[Node] = match &scope {
                Some(p) => find(&tree, p)
                    .and_then(|n| n.children.as_deref())
                    .ok_or_else(|| StoreError::NotFound(format!("{p} isn't a topic")))?,
                None => &tree,
            };
            let mut out = String::new();
            outline(nodes, 0, &mut out);
            if out.is_empty() {
                out = "(empty)".into();
            }
            Outcome::ok(format!("Listed {}", scope.as_deref().unwrap_or("all notes")), clip(out))
        }
        "read_note" => {
            let path = note_path(store, arg(args, "path")?);
            let doc = store.read(&path)?;
            let lines: Vec<&str> = doc.content.lines().collect();
            let start = num(args, "start_line").unwrap_or(1).max(1);
            let end = num(args, "end_line")
                .unwrap_or(lines.len())
                .min(lines.len())
                .min(start + MAX_READ_LINES - 1);
            let mut out = String::new();
            for (i, line) in lines.iter().enumerate().take(end).skip(start - 1) {
                out.push_str(&format!("{:>4}  {line}\n", i + 1));
            }
            if end < lines.len() {
                out.push_str(&format!("… {} more lines (read with start_line={})\n", lines.len() - end, end + 1));
            }
            Outcome::ok(
                format!("Read {path}"),
                json!({ "path": path, "total_lines": lines.len(), "text": clip(out) }),
            )
        }
        "search_notes" => {
            let query = arg(args, "query")?;
            let re = pattern(query, flag(args, "regex", false), flag(args, "case_sensitive", false))?;
            let mut hits = Vec::new();
            let mut total = 0;
            for (rel, text) in notes_under(store, opt(args, "path"))? {
                for (i, line) in text.lines().enumerate() {
                    if re.is_match(line) {
                        total += 1;
                        if hits.len() < MAX_MATCHES {
                            hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                        }
                    }
                }
            }
            let mut out = hits.join("\n");
            if total > hits.len() {
                out.push_str(&format!("\n… {} more matches", total - hits.len()));
            }
            if total == 0 {
                out = "No matches.".into();
            }
            Outcome::ok(format!("Searched for “{query}” · {total} found"), clip(out))
        }
        "create_note" => {
            let topic = args.get("topic").and_then(Value::as_str).unwrap_or("");
            let path = store.create_note(topic, arg(args, "title")?, arg(args, "content")?)?;
            Outcome::ok(format!("Created {path}"), json!({ "path": path })).changed([path])
        }
        "edit_note" => {
            let path = note_path(store, arg(args, "path")?);
            let (find, replace) = (arg(args, "find")?, arg(args, "replace")?);
            if find.is_empty() {
                bail!("'find' can't be empty");
            }
            let doc = store.read(&path)?;
            let count = doc.content.matches(find).count();
            let all = flag(args, "replace_all", false);
            match count {
                0 => bail!("'find' text isn't in {path}; read the note and copy the text exactly"),
                n if n > 1 && !all => bail!("'find' matches {n} places in {path}; add more context or set replace_all"),
                _ => {}
            }
            let content = if all {
                doc.content.replace(find, replace)
            } else {
                doc.content.replacen(find, replace, 1)
            };
            store.write(&path, &content, Some(doc.modified))?;
            let n = if all { count } else { 1 };
            Outcome::ok(format!("Edited {path}"), format!("Replaced {n} occurrence(s)")).changed([path])
        }
        "append_to_note" => {
            let path = note_path(store, arg(args, "path")?);
            let text = arg(args, "text")?;
            let doc = store.read(&path)?;
            let sep = if doc.content.is_empty() || doc.content.ends_with("\n\n") {
                ""
            } else if doc.content.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            store.append(&path, &format!("{sep}{}\n", text.trim_matches('\n').trim_end()))?;
            Outcome::ok(format!("Added to {path}"), "Appended").changed([path])
        }
        "write_note" => {
            let path = note_path(store, arg(args, "path")?);
            let doc = store.write(&path, arg(args, "content")?, None)?;
            Outcome::ok(format!("Wrote {}", doc.path), "Saved").changed([doc.path])
        }
        "replace_in_notes" => {
            let (pat, replacement) = (arg(args, "pattern")?, arg(args, "replacement")?);
            if pat.is_empty() {
                bail!("'pattern' can't be empty");
            }
            let regex = flag(args, "regex", false);
            let re = pattern(pat, regex, flag(args, "case_sensitive", true))?;
            let dry = flag(args, "dry_run", false);
            let mut report = Vec::new();
            let mut changed = Vec::new();
            let mut total = 0;
            for (rel, text) in notes_under(store, opt(args, "path"))? {
                let n = re.find_iter(&text).count();
                if n == 0 {
                    continue;
                }
                total += n;
                report.push(format!("{rel}: {n}"));
                if !dry {
                    let new = if regex {
                        re.replace_all(&text, replacement)
                    } else {
                        re.replace_all(&text, regex::NoExpand(replacement))
                    };
                    store.write(&rel, &new, None)?;
                    changed.push(rel);
                }
            }
            let verb = if dry { "Would replace" } else { "Replaced" };
            let out = if total == 0 {
                "No matches.".to_string()
            } else {
                format!("{verb} {total} match(es) in {} note(s):\n{}", report.len(), report.join("\n"))
            };
            Outcome::ok(format!("{verb} “{pat}” · {total} in {} notes", report.len()), clip(out)).changed(changed)
        }
        "create_topic" => {
            let parent = args.get("parent").and_then(Value::as_str).unwrap_or("");
            let path = store.create_topic(parent, arg(args, "name")?)?;
            Outcome::ok(format!("Created topic {path}"), json!({ "path": path })).changed([path])
        }
        "rename_item" => {
            let from = arg(args, "path")?.trim_matches('/');
            let from = if store.resolve(from)?.exists() {
                from.to_string()
            } else {
                note_path(store, from)
            };
            let to = store.rename(&from, arg(args, "new_name")?)?;
            Outcome::ok(format!("Renamed to {to}"), json!({ "path": to })).changed([from, to])
        }
        "move_item" => {
            let from = arg(args, "path")?.trim_matches('/');
            let from = if store.resolve(from)?.exists() {
                from.to_string()
            } else {
                note_path(store, from)
            };
            let to = store.move_to(&from, args.get("to_topic").and_then(Value::as_str).unwrap_or(""))?;
            Outcome::ok(format!("Moved to {to}"), json!({ "path": to })).changed([from, to])
        }
        "delete_item" => {
            let path = arg(args, "path")?.trim_matches('/');
            let path = if store.resolve(path)?.exists() {
                path.to_string()
            } else {
                note_path(store, path)
            };
            let entry = store.delete(&path)?;
            Outcome::ok(
                format!("Moved {path} to the trash"),
                "Moved to the trash; it can be restored from the app.",
            )
            .changed([entry.path])
        }
        "run_command" => {
            let command = arg(args, "command")?;
            let out = run_command(store.root(), command)?;
            Outcome::ok(format!("$ {command}"), clip(out))
        }
        other => bail!("unknown tool {other}"),
    })
}

fn find<'a>(nodes: &'a [Node], path: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|n| {
        if n.path == path {
            Some(n)
        } else {
            n.children.as_deref().and_then(|c| find(c, path))
        }
    })
}

fn outline(nodes: &[Node], depth: usize, out: &mut String) {
    for n in nodes {
        let pad = "  ".repeat(depth);
        match n.kind {
            Kind::Topic => {
                out.push_str(&format!("{pad}{}/   ({})\n", n.name, n.path));
                outline(n.children.as_deref().unwrap_or_default(), depth + 1, out);
            }
            Kind::Note => out.push_str(&format!("{pad}- {}   ({})\n", n.name, n.path)),
        }
    }
}

fn pattern(text: &str, regex: bool, case_sensitive: bool) -> Result<Regex> {
    let src = if regex { text.to_string() } else { regex::escape(text) };
    RegexBuilder::new(&src)
        .case_insensitive(!case_sensitive)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| anyhow!("bad pattern: {e}"))
}

/// Every note under `scope` (a topic or a single note), as (relative path, content).
fn notes_under(store: &Store, scope: Option<&str>) -> Result<Vec<(String, String)>> {
    let base = match scope {
        Some(p) => {
            let p = p.trim_matches('/');
            let abs = store.resolve(p)?;
            if abs.exists() { abs } else { store.resolve(&note_path(store, p))? }
        }
        None => store.root().to_path_buf(),
    };
    if !base.exists() {
        return Err(StoreError::NotFound(format!("{} doesn't exist", store.rel(&base))).into());
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(&base)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
    {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_file()
            && path.extension().is_some_and(|e| e == "md" || e == "txt")
            && let Ok(text) = std::fs::read_to_string(path)
        {
            out.push((store.rel(path), text));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn clip(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        let mut cut = MAX_OUTPUT;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n… (output cut; narrow the request)");
    }
    s
}

// ---- run_command ------------------------------------------------------------------------------

const ALLOWED: [&str; 12] = [
    "ls", "cat", "head", "tail", "wc", "grep", "sort", "uniq", "cut", "tr", "find", "diff",
];

/// Splits a command line into pipeline stages of words, honoring quotes. Refuses redirection,
/// command chaining and substitution.
pub fn parse_pipeline(line: &str) -> Result<Vec<Vec<String>>> {
    let mut stages = vec![Vec::new()];
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => word.push(ch),
                        None => bail!("unclosed ' quote"),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\')) => word.push(ch),
                            Some(ch) => {
                                word.push('\\');
                                word.push(ch);
                            }
                            None => bail!("unclosed \" quote"),
                        },
                        Some('$' | '`') => bail!("no variables or substitution"),
                        Some(ch) => word.push(ch),
                        None => bail!("unclosed \" quote"),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(ch) = chars.next() {
                    word.push(ch);
                }
            }
            '|' => {
                if in_word {
                    stages.last_mut().unwrap().push(std::mem::take(&mut word));
                    in_word = false;
                }
                if chars.peek() == Some(&'|') {
                    bail!("|| isn't allowed; run one pipeline at a time");
                }
                stages.push(Vec::new());
            }
            ';' | '&' | '>' | '<' | '`' | '$' | '(' | ')' | '\n' => {
                bail!("'{c}' isn't allowed; only simple commands joined with | (use the edit tools to change files)")
            }
            c if c.is_whitespace() => {
                if in_word {
                    stages.last_mut().unwrap().push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        stages.last_mut().unwrap().push(word);
    }
    if stages.iter().any(Vec::is_empty) {
        bail!("empty command");
    }
    Ok(stages)
}

/// Checks a stage against the allowlist and keeps every path argument inside the folder.
fn check_stage(words: &[String]) -> Result<()> {
    let program = words[0].as_str();
    if !ALLOWED.contains(&program) {
        bail!("{program} isn't available; allowed: {}", ALLOWED.join(", "));
    }
    for w in &words[1..] {
        let lower = w.to_ascii_lowercase();
        match program {
            "find"
                if [
                    "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls",
                ]
                .contains(&lower.as_str()) =>
            {
                bail!("find {w} isn't allowed")
            }
            "sort" if w == "-o" || w.starts_with("--output") || (w.starts_with('-') && !w.starts_with("--") && w.contains('o')) => {
                bail!("sort can't write files here")
            }
            _ => {}
        }
        if w.starts_with('-') {
            // Options like --file=/etc/x still name a path.
            if let Some((_, value)) = w.split_once('=') {
                check_path_word(value)?;
            }
            continue;
        }
        check_path_word(w)?;
    }
    Ok(())
}

fn check_path_word(w: &str) -> Result<()> {
    if w.starts_with('/') || w.starts_with('~') {
        bail!("{w}: use paths relative to the notes folder");
    }
    if Path::new(w).components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        bail!("{w}: paths can't go up with ..");
    }
    Ok(())
}

pub fn run_command(root: &Path, line: &str) -> Result<String> {
    let stages = parse_pipeline(line)?;
    for s in &stages {
        check_stage(s)?;
    }
    let mut children = Vec::new();
    let mut prev: Option<std::process::ChildStdout> = None;
    for words in &stages {
        let mut cmd = Command::new(&words[0]);
        cmd.args(&words[1..])
            .current_dir(root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/local/bin")
            .env("LC_ALL", "en_US.UTF-8")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.stdin(match prev.take() {
            Some(out) => Stdio::from(out),
            None => Stdio::null(),
        });
        let mut child = cmd.spawn().map_err(|e| anyhow!("{}: {e}", words[0]))?;
        prev = child.stdout.take();
        children.push(child);
    }
    let mut stdout = prev.ok_or_else(|| anyhow!("no output"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.by_ref().take((MAX_OUTPUT * 2) as u64).read_to_end(&mut buf);
        buf
    });
    let started = Instant::now();
    let mut timed_out = false;
    for child in children.iter_mut() {
        loop {
            match child.try_wait()? {
                Some(_) => break,
                None if started.elapsed() > COMMAND_TIMEOUT => {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
    let out = reader.join().unwrap_or_default();
    let mut err = String::new();
    if let Some(last) = children.last_mut()
        && let Some(mut e) = last.stderr.take()
    {
        let _ = e.by_ref().take(4000).read_to_string(&mut err);
    }
    let mut text = String::from_utf8_lossy(&out).into_owned();
    if timed_out {
        text.push_str("\n(stopped after 10 seconds)");
    }
    if text.trim().is_empty() {
        text = if err.trim().is_empty() { "(no output)".into() } else { err };
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        let sales = s.create_topic("", "Sales").unwrap();
        s.create_note(
            &sales,
            "Monday",
            "[10:00:01] Pune sales were 4.5 lakh.\n[10:00:09] नाशिक delayed.\n",
        )
        .unwrap();
        s.create_note(&sales, "Tuesday", "[11:00:00] pune target missed\n").unwrap();
        (dir, s)
    }

    fn call(s: &Store, name: &str, args: Value) -> Outcome {
        execute(s, &View::default(), name, &args)
    }

    #[test]
    fn declarations_are_valid() {
        let d = declarations();
        let names: Vec<&str> = d.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"replace_in_notes") && names.contains(&"run_command"));
        // Every declared tool is handled.
        let (_d, s) = setup();
        for n in names {
            let o = call(&s, n, json!({}));
            assert!(!o.summary.contains("unknown tool"), "{n} is not implemented");
        }
    }

    #[test]
    fn list_read_search() {
        let (_d, s) = setup();
        let o = call(&s, "list_notes", json!({}));
        let text = o.response["output"].as_str().unwrap();
        assert!(text.contains("Sales/") && text.contains("(Sales/Monday.md)"), "{text}");

        let o = call(&s, "read_note", json!({ "path": "Sales/Monday" }));
        assert!(o.ok, "{:?}", o.response);
        assert_eq!(o.response["output"]["total_lines"], 2);

        let o = call(&s, "search_notes", json!({ "query": "PUNE" }));
        let text = o.response["output"].as_str().unwrap();
        assert!(
            text.contains("Sales/Monday.md:1:") && text.contains("Sales/Tuesday.md:1:"),
            "{text}"
        );
        let o = call(&s, "search_notes", json!({ "query": "नाशिक" }));
        assert!(o.response["output"].as_str().unwrap().contains("Monday.md:2"));
    }

    #[test]
    fn edits() {
        let (_d, s) = setup();
        let o = call(
            &s,
            "edit_note",
            json!({ "path": "Sales/Monday.md", "find": "4.5 lakh", "replace": "45 lakh" }),
        );
        assert!(o.ok && o.changed == ["Sales/Monday.md"], "{:?}", o.response);
        assert!(s.read("Sales/Monday.md").unwrap().content.contains("45 lakh"));

        let o = call(
            &s,
            "edit_note",
            json!({ "path": "Sales/Monday.md", "find": "nothing here", "replace": "x" }),
        );
        assert!(!o.ok);

        let o = call(
            &s,
            "append_to_note",
            json!({ "path": "Sales/Monday.md", "text": "## Summary\nGood day." }),
        );
        assert!(o.ok);
        assert!(s.read("Sales/Monday.md").unwrap().content.ends_with("\n\n## Summary\nGood day.\n"));
    }

    #[test]
    fn replace_at_scale() {
        let (_d, s) = setup();
        let dry = call(
            &s,
            "replace_in_notes",
            json!({ "pattern": "pune", "replacement": "Pune", "case_sensitive": false, "dry_run": true }),
        );
        assert!(dry.changed.is_empty());
        assert!(dry.response["output"].as_str().unwrap().contains("Would replace 2"));
        let real = call(
            &s,
            "replace_in_notes",
            json!({ "pattern": r"\[(\d\d):(\d\d):\d\d\]", "replacement": "[$1:$2]", "regex": true }),
        );
        assert_eq!(real.changed.len(), 2);
        assert!(s.read("Sales/Tuesday.md").unwrap().content.starts_with("[11:00] pune"));
    }

    #[test]
    fn topics_and_trash() {
        let (_d, s) = setup();
        assert!(call(&s, "create_topic", json!({ "parent": "Sales", "name": "Nashik" })).ok);
        assert!(call(&s, "move_item", json!({ "path": "Sales/Tuesday", "to_topic": "Sales/Nashik" })).ok);
        assert!(s.read("Sales/Nashik/Tuesday.md").is_ok());
        assert!(call(&s, "rename_item", json!({ "path": "Sales/Nashik", "new_name": "Nashik office" })).ok);
        assert!(call(&s, "delete_item", json!({ "path": "Sales/Nashik office" })).ok);
        assert!(!s.resolve("Sales/Nashik office").unwrap().exists());
    }

    #[test]
    fn pipeline_parsing() {
        assert_eq!(
            parse_pipeline("grep -ri 'pune sales' . | wc -l").unwrap(),
            vec![vec!["grep", "-ri", "pune sales", "."], vec!["wc", "-l"]]
        );
        assert_eq!(parse_pipeline(r#"grep "a \"b\"" x"#).unwrap(), vec![vec!["grep", "a \"b\"", "x"]]);
        for bad in [
            "ls; rm -rf /",
            "cat x > y",
            "echo $(id)",
            "ls && ls",
            "cat `x`",
            "ls || ls",
            "grep x |",
        ] {
            assert!(parse_pipeline(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn command_allowlist() {
        let (_d, s) = setup();
        for bad in [
            "rm -rf .",
            "cat /etc/passwd",
            "cat ../x",
            "find . -delete",
            "sort -o out.md x",
            "grep --file=/etc/hosts .",
            "sh -c ls",
            "python3 -c 1",
        ] {
            assert!(run_command(s.root(), bad).is_err(), "{bad} should be refused");
        }
    }

    #[cfg(unix)]
    #[test]
    fn commands_run_in_folder() {
        let (_d, s) = setup();
        let out = run_command(s.root(), "grep -ri pune Sales | wc -l").unwrap();
        assert_eq!(out.trim(), "2");
        let out = run_command(s.root(), "ls Sales").unwrap();
        assert!(out.contains("Monday.md"));
    }
}
