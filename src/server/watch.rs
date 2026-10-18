//! Watches the data folder and tells the browser what changed, whatever changed it: the app, the
//! assistant, a recording, or the person editing a note in another editor.

use super::Shared;
use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const SETTLE: Duration = Duration::from_millis(150);

pub fn start(state: Shared) -> Result<RecommendedWatcher> {
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(state.store.root(), RecursiveMode::Recursive)?;
    std::thread::Builder::new().name("parrot-watch".into()).spawn(move || {
        let mut pending: BTreeSet<String> = BTreeSet::new();
        let mut since: Option<Instant> = None;
        loop {
            let wait = since
                .map(|t| SETTLE.saturating_sub(t.elapsed()))
                .unwrap_or(Duration::from_secs(3600));
            match rx.recv_timeout(wait) {
                Ok(Ok(event)) => {
                    for path in event.paths {
                        let rel = state.store.rel(&path);
                        if rel.split('/').any(|part| part.starts_with('.')) {
                            continue;
                        }
                        pending.insert(rel);
                        since.get_or_insert_with(Instant::now);
                    }
                }
                Ok(Err(_)) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if since.is_some_and(|t| t.elapsed() >= SETTLE) {
                state.changed(std::mem::take(&mut pending).into_iter().collect());
                since = None;
            }
        }
    })?;
    Ok(watcher)
}
