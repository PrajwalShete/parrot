//! The notebook on disk: topics are folders (nested for subtopics), notes and transcripts are
//! Markdown files. Plain files on purpose, so they open in any editor and the voice assistant can
//! search and edit them like a person would.
//!
//! Every path from outside (the browser, the assistant) is relative to the data folder and goes
//! through [`Store::resolve`], which refuses anything that would leave it. Deleting moves things to
//! `.trash` so they can be restored.

use anyhow::{Context, Result};
use serde::Serialize;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const TRASH: &str = ".trash";
const NOTE_EXTS: [&str; 2] = ["md", "txt"];
const MAX_NAME: usize = 120;

/// Errors the HTTP layer maps to status codes.
#[derive(Debug, PartialEq)]
pub enum StoreError {
    NotFound(String),
    Invalid(String),
    Conflict(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound(m) | StoreError::Invalid(m) | StoreError::Conflict(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for StoreError {}

fn not_found(path: &str) -> anyhow::Error {
    StoreError::NotFound(format!("{path} doesn't exist")).into()
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    StoreError::Invalid(msg.into()).into()
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Topic,
    Note,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub name: String,
    /// Relative to the data folder, always with `/`.
    pub path: String,
    pub kind: Kind,
    /// Milliseconds since the epoch.
    pub modified: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Node>>,
}

#[derive(Debug, Serialize)]
pub struct Doc {
    pub path: String,
    pub name: String,
    pub content: String,
    pub modified: i64,
}

#[derive(Debug, Serialize)]
pub struct TrashEntry {
    pub id: String,
    pub path: String,
}

pub struct Store {
    root: PathBuf,
    /// Serializes writes from the browser, the transcriber and the assistant.
    lock: Mutex<()>,
}

impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        let root = root.canonicalize().with_context(|| format!("opening {}", root.display()))?;
        Ok(Self {
            root,
            lock: Mutex::new(()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- Paths ----------------------------------------------------------------------------

    /// Turns a relative path from outside into a path inside the data folder, or refuses it.
    /// `""` is the data folder itself. Hidden names (`.trash`, `.git`) can't be addressed.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let mut out = self.root.clone();
        for part in rel.split(['/', '\\']) {
            match part {
                "" | "." => continue,
                ".." => return Err(invalid(format!("{rel}: paths can't go up with .."))),
                p if p.starts_with('.') => return Err(invalid(format!("{rel}: hidden files are off limits"))),
                p => {
                    if Path::new(p).components().any(|c| !matches!(c, Component::Normal(_))) {
                        return Err(invalid(format!("{rel}: not a relative path")));
                    }
                    out.push(p);
                }
            }
        }
        // A symlink inside the folder must not lead out of it.
        let mut probe = out.as_path();
        loop {
            if let Ok(real) = probe.canonicalize() {
                if !real.starts_with(&self.root) {
                    return Err(invalid(format!("{rel}: points outside the Parrot folder")));
                }
                break;
            }
            match probe.parent() {
                Some(p) => probe = p,
                None => break,
            }
        }
        Ok(out)
    }

    /// The relative, `/`-separated form of a path inside the folder.
    pub fn rel(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.root)
            .unwrap_or(abs)
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    fn existing(&self, rel: &str) -> Result<PathBuf> {
        let path = self.resolve(rel)?;
        if !path.exists() {
            return Err(not_found(rel));
        }
        Ok(path)
    }

    fn topic(&self, rel: &str) -> Result<PathBuf> {
        let path = self.existing(rel)?;
        if !path.is_dir() {
            return Err(invalid(format!("{rel} is not a topic")));
        }
        Ok(path)
    }

    fn note(&self, rel: &str) -> Result<PathBuf> {
        let path = self.existing(rel)?;
        if !is_note(&path) {
            return Err(invalid(format!("{rel} is not a note")));
        }
        Ok(path)
    }

    // ---- Reading --------------------------------------------------------------------------

    /// Everything in the folder: topics first (A–Z), then notes (newest first), recursively.
    pub fn tree(&self) -> Result<Vec<Node>> {
        self.children(&self.root)
    }

    fn children(&self, dir: &Path) -> Result<Vec<Node>> {
        let mut topics = Vec::new();
        let mut notes = Vec::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let modified = millis(meta.modified().ok());
            if meta.is_dir() {
                // Don't follow links out of the folder.
                if path.canonicalize().map(|p| !p.starts_with(&self.root)).unwrap_or(true) {
                    continue;
                }
                topics.push(Node {
                    name,
                    path: self.rel(&path),
                    kind: Kind::Topic,
                    modified,
                    size: None,
                    children: Some(self.children(&path)?),
                });
            } else if is_note(&path) {
                notes.push(Node {
                    name: display_name(&path),
                    path: self.rel(&path),
                    kind: Kind::Note,
                    modified,
                    size: Some(meta.len()),
                    children: None,
                });
            }
        }
        topics.sort_by_key(|n| n.name.to_lowercase());
        notes.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
        topics.extend(notes);
        Ok(topics)
    }

    pub fn read(&self, rel: &str) -> Result<Doc> {
        let path = self.note(rel)?;
        let content = std::fs::read_to_string(&path).with_context(|| format!("reading {rel}"))?;
        Ok(Doc {
            path: self.rel(&path),
            name: display_name(&path),
            content,
            modified: modified(&path),
        })
    }

    // ---- Writing --------------------------------------------------------------------------

    pub fn create_topic(&self, parent: &str, name: &str) -> Result<String> {
        let _g = self.guard();
        let dir = self.topic(parent)?;
        let name = clean_name(name)?;
        let path = unique(&dir, &name, None);
        std::fs::create_dir(&path).with_context(|| format!("creating {}", path.display()))?;
        Ok(self.rel(&path))
    }

    /// Creates `title.md` in `parent` (a free name: "Standup 2.md" if taken) holding `content`.
    pub fn create_note(&self, parent: &str, title: &str, content: &str) -> Result<String> {
        let _g = self.guard();
        let dir = self.topic(parent)?;
        let title = clean_name(strip_ext(title))?;
        let path = unique(&dir, &title, Some("md"));
        write_atomic(&path, content)?;
        Ok(self.rel(&path))
    }

    /// Replaces a note. With `expected` (the `modified` the editor loaded), refuses if the file
    /// changed since, so a browser tab never silently overwrites the assistant or a recording.
    pub fn write(&self, rel: &str, content: &str, expected: Option<i64>) -> Result<Doc> {
        let _g = self.guard();
        let path = self.resolve(rel)?;
        if path.exists() && !is_note(&path) {
            return Err(invalid(format!("{rel} is not a note")));
        }
        if !path.exists() && !has_note_ext(&path) {
            return Err(invalid(format!("{rel}: notes end in .md")));
        }
        if let Some(expected) = expected
            && path.exists()
            && modified(&path) != expected
        {
            return Err(StoreError::Conflict(format!("{rel} changed since it was opened")).into());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(&path, content)?;
        Ok(Doc {
            path: self.rel(&path),
            name: display_name(&path),
            content: content.to_string(),
            modified: modified(&path),
        })
    }

    pub fn append(&self, rel: &str, text: &str) -> Result<i64> {
        use std::io::Write;
        let _g = self.guard();
        let path = self.note(rel)?;
        let mut f = std::fs::OpenOptions::new().append(true).open(&path)?;
        f.write_all(text.as_bytes())?;
        f.sync_data().ok();
        Ok(modified(&path))
    }

    /// Renames a topic or note in place. Notes keep their extension.
    pub fn rename(&self, rel: &str, new_name: &str) -> Result<String> {
        let _g = self.guard();
        let path = self.existing(rel)?;
        if path == self.root {
            return Err(invalid("the Parrot folder itself can't be renamed"));
        }
        let parent = path.parent().context("no parent")?;
        let ext = if path.is_dir() {
            None
        } else {
            path.extension().map(|e| e.to_string_lossy().into_owned())
        };
        let name = clean_name(strip_ext(new_name))?;
        let target = match &ext {
            Some(e) => parent.join(format!("{name}.{e}")),
            None => parent.join(&name),
        };
        if target == path {
            return Ok(self.rel(&path));
        }
        // Allow case-only renames on case-insensitive disks.
        let same_file = target.exists() && same_path_ignoring_case(&target, &path);
        if target.exists() && !same_file {
            return Err(StoreError::Conflict(format!("{} already exists", self.rel(&target))).into());
        }
        std::fs::rename(&path, &target)?;
        Ok(self.rel(&target))
    }

    /// Moves a topic or note into another topic (`""` = top level).
    pub fn move_to(&self, rel: &str, new_parent: &str) -> Result<String> {
        let _g = self.guard();
        let path = self.existing(rel)?;
        let dest = self.topic(new_parent)?;
        if path == self.root {
            return Err(invalid("the Parrot folder itself can't be moved"));
        }
        if path.is_dir() && dest.starts_with(&path) {
            return Err(invalid("a topic can't move inside itself"));
        }
        if path.parent() == Some(dest.as_path()) {
            return Ok(self.rel(&path));
        }
        let file = path.file_name().context("no name")?.to_string_lossy().into_owned();
        let (stem, ext) = if path.is_dir() {
            (file.clone(), None)
        } else {
            (display_name(&path), path.extension().map(|e| e.to_string_lossy().into_owned()))
        };
        let target = unique(&dest, &stem, ext.as_deref());
        std::fs::rename(&path, &target)?;
        Ok(self.rel(&target))
    }

    /// Moves a topic or note to `.trash` and returns how to restore it.
    pub fn delete(&self, rel: &str) -> Result<TrashEntry> {
        let _g = self.guard();
        let path = self.existing(rel)?;
        if path == self.root {
            return Err(invalid("the Parrot folder itself can't be deleted"));
        }
        let id = format!("{}-{}", now_millis(), std::process::id() % 10_000);
        let slot = self.root.join(TRASH).join(&id);
        std::fs::create_dir_all(&slot)?;
        let original = self.rel(&path);
        std::fs::write(slot.join(".origin"), &original)?;
        let name = path.file_name().context("no name")?;
        std::fs::rename(&path, slot.join(name))?;
        Ok(TrashEntry { id, path: original })
    }

    /// Puts a deleted item back where it was (under a free name if that's taken now).
    pub fn restore(&self, id: &str) -> Result<String> {
        let _g = self.guard();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Err(invalid("bad trash id"));
        }
        let slot = self.root.join(TRASH).join(id);
        let original = std::fs::read_to_string(slot.join(".origin")).map_err(|_| not_found(id))?;
        let item = std::fs::read_dir(&slot)?
            .filter_map(|e| e.ok())
            .find(|e| e.file_name() != ".origin")
            .ok_or_else(|| not_found(id))?
            .path();
        let wanted = self.resolve(&original)?;
        let parent = wanted.parent().context("no parent")?.to_path_buf();
        std::fs::create_dir_all(&parent)?;
        let target = if wanted.exists() {
            let ext = if item.is_dir() {
                None
            } else {
                item.extension().map(|e| e.to_string_lossy().into_owned())
            };
            unique(&parent, &display_name(&item), ext.as_deref())
        } else {
            wanted
        };
        std::fs::rename(&item, &target)?;
        let _ = std::fs::remove_dir_all(&slot);
        Ok(self.rel(&target))
    }
}

// ---- Helpers ----------------------------------------------------------------------------------

/// A name safe on every OS: no separators or reserved characters, no leading dots, not too long.
pub fn clean_name(name: &str) -> Result<String> {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_start_matches('.').trim().to_string();
    let cleaned: String = cleaned.chars().take(MAX_NAME).collect::<String>().trim_end().to_string();
    if cleaned.is_empty() {
        return Err(invalid("a name can't be empty"));
    }
    Ok(cleaned)
}

fn strip_ext(name: &str) -> &str {
    for ext in NOTE_EXTS {
        if let Some(stem) = name.strip_suffix(&format!(".{ext}")) {
            return stem;
        }
    }
    name
}

fn has_note_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| NOTE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn is_note(path: &Path) -> bool {
    path.is_file() && has_note_ext(path)
}

/// File name without the note extension.
fn display_name(path: &Path) -> String {
    let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    if path.is_dir() { file } else { strip_ext(&file).to_string() }
}

/// `dir/name[.ext]`, or `dir/name 2[.ext]`, `name 3`, ... if taken.
fn unique(dir: &Path, name: &str, ext: Option<&str>) -> PathBuf {
    let make = |n: usize| {
        let base = if n == 1 { name.to_string() } else { format!("{name} {n}") };
        match ext {
            Some(e) => dir.join(format!("{base}.{e}")),
            None => dir.join(base),
        }
    };
    (1..).map(make).find(|p| !p.exists()).expect("a free name")
}

fn same_path_ignoring_case(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().context("no parent")?;
    let tmp = dir.join(format!(".{}.parrot-tmp", std::process::id()));
    std::fs::write(&tmp, content).with_context(|| format!("writing {}", path.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

pub fn modified(path: &Path) -> i64 {
    millis(std::fs::metadata(path).and_then(|m| m.modified()).ok())
}

fn millis(t: Option<SystemTime>) -> i64 {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_millis() -> i64 {
    millis(Some(SystemTime::now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    fn kind(e: anyhow::Error) -> StoreError {
        e.downcast::<StoreError>().expect("a StoreError")
    }

    #[test]
    fn refuses_paths_outside() {
        let (_d, s) = store();
        for bad in ["../x", "a/../../x", ".trash/x", "a/.git", "/etc/passwd/../..", "C:\\..\\x"] {
            assert!(s.resolve(bad).is_err(), "{bad} should be refused");
        }
        assert_eq!(s.resolve("").unwrap(), s.root());
        assert_eq!(s.resolve("/Sales//Q3.md").unwrap(), s.root().join("Sales").join("Q3.md"));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_out() {
        let (_d, s) = store();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), s.root().join("link")).unwrap();
        assert!(s.resolve("link/secret.md").is_err());
        assert!(s.tree().unwrap().is_empty(), "linked folders stay out of the tree");
    }

    #[test]
    fn topics_notes_and_tree() {
        let (_d, s) = store();
        let sales = s.create_topic("", "Sales").unwrap();
        let pune = s.create_topic(&sales, "Pune").unwrap();
        assert_eq!(pune, "Sales/Pune");
        let note = s.create_note(&pune, "Daily standup", "# hi\n").unwrap();
        assert_eq!(note, "Sales/Pune/Daily standup.md");
        assert_eq!(
            s.create_note(&pune, "Daily standup.md", "").unwrap(),
            "Sales/Pune/Daily standup 2.md"
        );
        s.create_topic("", "accounts").unwrap();

        let tree = s.tree().unwrap();
        assert_eq!(tree.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["accounts", "Sales"]);
        let pune_node = &tree[1].children.as_ref().unwrap()[0];
        assert_eq!(pune_node.kind, Kind::Topic);
        assert_eq!(pune_node.children.as_ref().unwrap().len(), 2);
        assert_eq!(s.read(&note).unwrap().content, "# hi\n");
    }

    #[test]
    fn write_detects_conflicts() {
        let (_d, s) = store();
        let note = s.create_note("", "n", "a").unwrap();
        let doc = s.read(&note).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        s.append(&note, "b").unwrap();
        let err = s.write(&note, "c", Some(doc.modified - 1)).unwrap_err();
        assert!(matches!(kind(err), StoreError::Conflict(_)));
        let fresh = s.read(&note).unwrap();
        assert_eq!(fresh.content, "ab");
        s.write(&note, "c", Some(fresh.modified)).unwrap();
        assert_eq!(s.read(&note).unwrap().content, "c");
        assert!(s.write("Sales.json", "{}", None).is_err(), "only notes can be written");
    }

    #[test]
    fn rename_move_delete_restore() {
        let (_d, s) = store();
        let a = s.create_topic("", "A").unwrap();
        let b = s.create_topic("", "B").unwrap();
        let note = s.create_note(&a, "Plan", "x").unwrap();

        let renamed = s.rename(&note, "Roadmap").unwrap();
        assert_eq!(renamed, "A/Roadmap.md");
        let moved = s.move_to(&renamed, &b).unwrap();
        assert_eq!(moved, "B/Roadmap.md");
        assert!(matches!(kind(s.move_to(&a, &a).unwrap_err()), StoreError::Invalid(_)));

        let trashed = s.delete(&b).unwrap();
        assert!(matches!(kind(s.read(&moved).unwrap_err()), StoreError::NotFound(_)));
        assert_eq!(s.tree().unwrap().len(), 1, ".trash is hidden");
        assert_eq!(s.restore(&trashed.id).unwrap(), "B");
        assert_eq!(s.read(&moved).unwrap().content, "x");
    }

    #[test]
    fn rename_refuses_collisions() {
        let (_d, s) = store();
        s.create_topic("", "A").unwrap();
        s.create_topic("", "B").unwrap();
        assert!(matches!(kind(s.rename("A", "B").unwrap_err()), StoreError::Conflict(_)));
        assert_eq!(s.rename("A", "a").unwrap(), "a");
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("  Q3: sales / review?  ").unwrap(), "Q3 sales review");
        assert_eq!(clean_name("..hidden").unwrap(), "hidden");
        assert_eq!(clean_name("मराठी बैठक").unwrap(), "मराठी बैठक");
        assert!(clean_name(" / ").is_err());
    }
}
