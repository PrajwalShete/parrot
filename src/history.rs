//! Version history for notes.
//!
//! Every change to a note is kept as a version: edits in the app, every edit the assistant makes
//! (even a single word), each recording, and restores. Versions live in `.history/` inside the
//! notebook, mirroring its folders (`.history/Sales/Standup.md/`), so a note's history moves with
//! it when it's renamed or moved. Each version is two files: `<id>.md` (the full text) and
//! `<id>.json` (who, when, what, and how many lines changed).
//!
//! Restoring never deletes anything: it writes the old text as a new version.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DIR: &str = ".history";
/// Versions kept per note; the oldest go first, but the very first one is always kept.
const KEEP: usize = 300;
/// Your own typing autosaves every few hundred milliseconds; edits within this window (with no
/// one else changing the note in between) become one version.
const COALESCE_MS: i64 = 2 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Author {
    /// The note as it was before Parrot first kept a version of it.
    Original,
    /// Edited in the app.
    You,
    /// Changed by the voice assistant.
    Assistant,
    /// Lines added by a live recording.
    Recording,
    /// An earlier version made current again.
    Restore,
    /// Changed outside Parrot (another editor), noticed before the next change.
    External,
}

/// Who changed a note and why, passed along with every write.
#[derive(Clone, Copy, Debug)]
pub struct Change<'a> {
    pub author: Author,
    pub summary: &'a str,
}

impl<'a> Change<'a> {
    pub fn new(author: Author, summary: &'a str) -> Self {
        Self { author, summary }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    /// Milliseconds since the epoch.
    pub created: i64,
    pub author: Author,
    pub summary: String,
    /// Lines added and removed compared with the version before it.
    pub added: usize,
    pub removed: usize,
    /// Bytes.
    pub size: usize,
}

pub struct History {
    root: PathBuf,
}

impl History {
    pub fn new(notebook: &Path) -> Self {
        Self { root: notebook.join(DIR) }
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let mut d = self.root.clone();
        for part in rel.split('/').filter(|p| !p.is_empty()) {
            d.push(part);
        }
        d
    }

    /// Versions of a note, newest first.
    pub fn list(&self, rel: &str) -> Result<Vec<Version>> {
        let dir = self.dir(rel);
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(out) };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json")
                && let Ok(v) = std::fs::read_to_string(&p)
                    .map_err(anyhow::Error::from)
                    .and_then(|t| Ok(serde_json::from_str::<Version>(&t)?))
            {
                out.push(v);
            }
        }
        out.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(out)
    }

    /// One version's metadata and full text.
    pub fn get(&self, rel: &str, id: &str) -> Result<(Version, String)> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
            anyhow::bail!("bad version id");
        }
        let dir = self.dir(rel);
        let meta: Version = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{id}.json"))).context("no such version")?)?;
        let text = std::fs::read_to_string(dir.join(format!("{id}.md"))).context("version text is missing")?;
        Ok((meta, text))
    }

    fn latest(&self, rel: &str) -> Option<(Version, String)> {
        let v = self.list(rel).ok()?.into_iter().next()?;
        let text = std::fs::read_to_string(self.dir(rel).join(format!("{}.md", v.id))).ok()?;
        Some((v, text))
    }

    /// Records `after` as the newest version of `rel`.
    ///
    /// `before` is what the file held just before this change. If history doesn't know that text
    /// (the first change ever, or someone edited the file elsewhere), it's kept first, so every
    /// state the note was in can be restored. Returns None when nothing changed.
    pub fn record(&self, rel: &str, before: Option<&str>, after: &str, change: Change) -> Result<Option<Version>> {
        let latest = self.latest(rel);
        let mut previous_text = latest.as_ref().map(|(_, t)| t.clone());
        if let Some(before) = before {
            match &latest {
                None if !before.is_empty() => {
                    self.save(rel, Author::Original, "Before Parrot kept versions", None, before)?;
                    previous_text = Some(before.to_string());
                }
                Some((_, text)) if text != before => {
                    self.save(rel, Author::External, "Changed outside Parrot", Some(text), before)?;
                    previous_text = Some(before.to_string());
                }
                _ => {}
            }
        }
        if previous_text.as_deref() == Some(after) {
            return Ok(None);
        }
        // Fold rapid autosaves of your own typing into one version.
        if change.author == Author::You
            && let Some((last, _)) = self.latest(rel)
        {
            let fresh = now() - last.created < COALESCE_MS;
            // Only autosaves merge; never into "Created" or other kinds of version.
            if fresh && last.author == Author::You && last.summary == change.summary {
                let versions = self.list(rel)?;
                let base = match versions.get(1) {
                    Some(v) => std::fs::read_to_string(self.dir(rel).join(format!("{}.md", v.id))).ok(),
                    None => None,
                };
                self.remove(rel, &last.id);
                return self.save(rel, change.author, change.summary, base.as_deref(), after).map(Some);
            }
        }
        self.save(rel, change.author, change.summary, previous_text.as_deref(), after)
            .map(Some)
    }

    fn save(&self, rel: &str, author: Author, summary: &str, previous: Option<&str>, text: &str) -> Result<Version> {
        let dir = self.dir(rel);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let created = now();
        // Sortable and unique even for several versions in one millisecond.
        let mut seq = 0;
        let id = loop {
            let id = format!("{created:013}-{seq:03}");
            if !dir.join(format!("{id}.json")).exists() {
                break id;
            }
            seq += 1;
        };
        let (added, removed) = line_changes(previous.unwrap_or(""), text);
        let v = Version {
            id: id.clone(),
            created,
            author,
            summary: summary.to_string(),
            added,
            removed,
            size: text.len(),
        };
        std::fs::write(dir.join(format!("{id}.md")), text)?;
        std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string_pretty(&v)?)?;
        self.prune(rel)?;
        Ok(v)
    }

    fn remove(&self, rel: &str, id: &str) {
        let dir = self.dir(rel);
        let _ = std::fs::remove_file(dir.join(format!("{id}.md")));
        let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
    }

    fn prune(&self, rel: &str) -> Result<()> {
        let versions = self.list(rel)?;
        if versions.len() <= KEEP {
            return Ok(());
        }
        // Newest first: keep the newest KEEP - 1 and the very first.
        for v in &versions[KEEP - 1..versions.len() - 1] {
            self.remove(rel, &v.id);
        }
        Ok(())
    }

    /// Moves a note's (or a whole topic's) history after a rename or move.
    pub fn moved(&self, from: &str, to: &str) -> Result<()> {
        let (src, dst) = (self.dir(from), self.dir(to));
        if !src.exists() || src == dst {
            return Ok(());
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if dst.exists() {
            // A case-only rename on a case-insensitive disk, or leftovers: keep the newer history.
            let _ = std::fs::remove_dir_all(&dst);
        }
        std::fs::rename(&src, &dst)?;
        Ok(())
    }

    /// Takes a note's history out (it goes to the trash with the note).
    pub fn take(&self, rel: &str, into: &Path) -> Result<()> {
        let src = self.dir(rel);
        if src.exists() {
            std::fs::rename(&src, into)?;
        }
        Ok(())
    }

    /// Puts history back after the note was restored from the trash.
    pub fn put(&self, rel: &str, from: &Path) -> Result<()> {
        if from.exists() {
            let dst = self.dir(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::remove_dir_all(&dst);
            std::fs::rename(from, &dst)?;
        }
        Ok(())
    }
}

/// Lines added and removed between two texts.
pub fn line_changes(before: &str, after: &str) -> (usize, usize) {
    let diff = similar::TextDiff::from_lines(before, after);
    let (mut added, mut removed) = (0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (added, removed)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> (tempfile::TempDir, History) {
        let d = tempfile::tempdir().unwrap();
        let h = History::new(d.path());
        (d, h)
    }

    #[test]
    fn keeps_the_original_then_each_change() {
        let (_d, h) = h();
        let v = h
            .record(
                "A/n.md",
                Some("one\n"),
                "one\ntwo\n",
                Change::new(Author::Assistant, "Added a line"),
            )
            .unwrap()
            .unwrap();
        assert_eq!((v.added, v.removed), (1, 0));
        let list = h.list("A/n.md").unwrap();
        assert_eq!(
            list.iter().map(|v| v.author).collect::<Vec<_>>(),
            [Author::Assistant, Author::Original]
        );
        assert_eq!(h.get("A/n.md", &list[1].id).unwrap().1, "one\n");
        // A single-word change by the assistant is its own version.
        let v = h
            .record(
                "A/n.md",
                Some("one\ntwo\n"),
                "one\nTwo\n",
                Change::new(Author::Assistant, "Capitalized"),
            )
            .unwrap()
            .unwrap();
        assert_eq!((v.added, v.removed), (1, 1));
        assert_eq!(h.list("A/n.md").unwrap().len(), 3);
    }

    #[test]
    fn skips_no_op_writes() {
        let (_d, h) = h();
        h.record("n.md", None, "x", Change::new(Author::You, "Created")).unwrap();
        assert!(
            h.record("n.md", Some("x"), "x", Change::new(Author::Assistant, "Nothing"))
                .unwrap()
                .is_none()
        );
        assert_eq!(h.list("n.md").unwrap().len(), 1);
    }

    #[test]
    fn notices_outside_edits() {
        let (_d, h) = h();
        h.record("n.md", None, "a", Change::new(Author::You, "Created")).unwrap();
        // Someone changed the file in another editor to "b", then the assistant writes "c".
        h.record("n.md", Some("b"), "c", Change::new(Author::Assistant, "Edit")).unwrap();
        let authors: Vec<Author> = h.list("n.md").unwrap().iter().map(|v| v.author).collect();
        assert_eq!(authors, [Author::Assistant, Author::External, Author::You]);
    }

    #[test]
    fn coalesces_your_typing_but_not_the_assistant() {
        let (_d, h) = h();
        h.record("n.md", None, "a", Change::new(Author::You, "Created")).unwrap();
        h.record("n.md", Some("a"), "ab", Change::new(Author::You, "Edited")).unwrap();
        h.record("n.md", Some("ab"), "abc", Change::new(Author::You, "Edited")).unwrap();
        let list = h.list("n.md").unwrap();
        assert_eq!(list.len(), 2, "two autosaves become one version");
        assert_eq!(h.get("n.md", &list[0].id).unwrap().1, "abc");
        h.record("n.md", Some("abc"), "abcd", Change::new(Author::Assistant, "Edit"))
            .unwrap();
        h.record("n.md", Some("abcd"), "abcde", Change::new(Author::You, "Edited")).unwrap();
        assert_eq!(h.list("n.md").unwrap().len(), 4, "typing after the assistant starts a new version");
    }

    #[test]
    fn history_follows_moves() {
        let (_d, h) = h();
        h.record("Sales/n.md", None, "a", Change::new(Author::You, "Created")).unwrap();
        h.moved("Sales", "Revenue").unwrap();
        assert!(h.list("Sales/n.md").unwrap().is_empty());
        assert_eq!(h.list("Revenue/n.md").unwrap().len(), 1);
    }

    #[test]
    fn rejects_bad_ids() {
        let (_d, h) = h();
        assert!(h.get("n.md", "../../etc").is_err());
    }
}
