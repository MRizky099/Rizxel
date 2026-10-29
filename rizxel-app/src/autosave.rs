//! Autosave and crash recovery.
//!
//! Dirty documents are written to `recovery/<key>.rzxl` (plain `.rzxl` v3
//! through the atomic `save_rzxl`) with a `recovery/<key>.json` describing
//! the original document. The project is snapshotted on the UI thread and
//! written on a background thread. A manual save, closing the document and
//! a clean exit delete the files; whatever is left on the next start was
//! interrupted and is offered for recovery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::paths::AppPaths;

/// By default, autosave a dirty document after this long...
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);
/// ...or after this many committed edits, whichever comes first.
pub const AUTOSAVE_EDITS: u64 = 50;

/// Sidecar of a recovery file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryMeta {
    /// Where the document was saved, `None` for an untitled one.
    pub original: Option<PathBuf>,
    /// Document name (untitled ones keep theirs).
    pub name: String,
    /// Tab title when it was written.
    pub title: String,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
}

/// A recovery file found on start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    /// File stem shared by the `.rzxl` and the `.json`.
    pub key: String,
    pub meta: RecoveryMeta,
}

impl RecoveryEntry {
    pub fn rzxl(&self, paths: &AppPaths) -> PathBuf {
        rzxl_path(paths, &self.key)
    }
}

fn rzxl_path(paths: &AppPaths, key: &str) -> PathBuf {
    paths.recovery_dir().join(format!("{key}.rzxl"))
}

fn meta_path(paths: &AppPaths, key: &str) -> PathBuf {
    paths.recovery_dir().join(format!("{key}.json"))
}

/// Deletes both files of `key` (missing ones are fine).
pub fn remove_recovery(paths: &AppPaths, key: &str) {
    for p in [rzxl_path(paths, key), meta_path(paths, key)] {
        if let Err(e) = std::fs::remove_file(&p) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("remove {}: {e}", p.display());
            }
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn modified_secs(path: &Path) -> Option<u64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Recovery files left by earlier sessions, newest first. Entries older
/// than their original file (it was saved later) are stale and deleted.
pub fn recoverable(paths: &AppPaths) -> Vec<RecoveryEntry> {
    let Ok(dir) = std::fs::read_dir(paths.recovery_dir()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Some(key) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let meta = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<RecoveryMeta>(&b).ok());
        let Some(meta) = meta.filter(|_| rzxl_path(paths, &key).is_file()) else {
            continue;
        };
        let stale = meta
            .original
            .as_deref()
            .and_then(modified_secs)
            .is_some_and(|m| m > meta.saved_at);
        if stale {
            remove_recovery(paths, &key);
            continue;
        }
        out.push(RecoveryEntry { key, meta });
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.meta.saved_at));
    out
}

/// Per-document schedule state.
#[derive(Debug, Clone, Copy)]
struct Mark {
    /// When the clock started (last autosave, or the document turning dirty).
    at: Instant,
    /// [`Document::edit_count`] at that time.
    edits: u64,
    /// Revision last written, so an unchanged document is not rewritten.
    written: Option<u64>,
}

/// Autosave scheduler plus the background writes in flight.
#[derive(Debug, Default)]
pub struct Autosave {
    marks: HashMap<u64, Mark>,
    /// Recovery file key per document id. New documents get
    /// `<process id>-<document id>`; restored ones keep their old key so the
    /// file they came from is overwritten, then deleted on save or close.
    keys: HashMap<u64, String>,
    writes: Vec<(u64, JoinHandle<Result<(), String>>)>,
    /// Time between autosaves (*Settings*): `None` for
    /// [`AUTOSAVE_INTERVAL`], `Some(None)` when autosave is off.
    interval: Option<Option<Duration>>,
}

impl Autosave {
    /// Sets the time between autosaves; `None` turns autosave off.
    pub fn set_interval(&mut self, interval: Option<Duration>) {
        self.interval = Some(interval);
    }

    fn interval(&self) -> Option<Duration> {
        self.interval.unwrap_or(Some(AUTOSAVE_INTERVAL))
    }

    pub fn key(&mut self, doc: u64) -> String {
        self.keys
            .entry(doc)
            .or_insert_with(|| format!("{}-{doc}", std::process::id()))
            .clone()
    }

    pub fn adopt_key(&mut self, doc: u64, key: String) {
        self.keys.insert(doc, key);
    }

    /// Documents due for an autosave at `now`: dirty, not mid-edit, changed
    /// since their last autosave, and past [`AUTOSAVE_INTERVAL`] or
    /// [`AUTOSAVE_EDITS`] since then. Nothing while autosave is off.
    pub fn due(&mut self, now: Instant, docs: &[Document]) -> Vec<u64> {
        let Some(interval) = self.interval() else {
            return Vec::new();
        };
        self.marks
            .retain(|id, _| docs.iter().any(|d| d.id() == *id));
        let mut due = Vec::new();
        for d in docs {
            let fresh = Mark {
                at: now,
                edits: d.edit_count(),
                written: None,
            };
            let mark = self.marks.entry(d.id()).or_insert(fresh);
            if !d.is_dirty() {
                // The clock starts when the document turns dirty.
                *mark = fresh;
                continue;
            }
            if d.is_editing() || mark.written == Some(d.revision()) {
                continue;
            }
            let elapsed = now.saturating_duration_since(mark.at) >= interval;
            let edits = d.edit_count().saturating_sub(mark.edits) >= AUTOSAVE_EDITS;
            if elapsed || edits {
                due.push(d.id());
            }
        }
        due
    }

    /// Snapshots `doc` and writes it on a background thread.
    pub fn write(&mut self, now: Instant, doc: &Document, paths: &AppPaths) {
        if self.writes.iter().any(|(id, _)| *id == doc.id()) {
            return;
        }
        let key = self.key(doc.id());
        self.marks.insert(
            doc.id(),
            Mark {
                at: now,
                edits: doc.edit_count(),
                written: Some(doc.revision()),
            },
        );
        let project = doc.to_project();
        let meta = RecoveryMeta {
            original: doc.path.clone(),
            name: doc.name.clone(),
            title: doc.title(),
            saved_at: unix_now(),
        };
        let (rzxl, json) = (rzxl_path(paths, &key), meta_path(paths, &key));
        let dir = paths.recovery_dir();
        let handle = std::thread::spawn(move || {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            rizxel_io::save_rzxl(&rzxl, &project).map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
            std::fs::write(&json, bytes).map_err(|e| e.to_string())
        });
        self.writes.push((doc.id(), handle));
    }

    /// Collects finished writes (`block` waits for all of them) as
    /// `(document id, result)`.
    pub fn finished(&mut self, block: bool) -> Vec<(u64, Result<(), String>)> {
        let mut done = Vec::new();
        let mut i = 0;
        while i < self.writes.len() {
            if block || self.writes[i].1.is_finished() {
                let (id, handle) = self.writes.swap_remove(i);
                let result = handle
                    .join()
                    .unwrap_or_else(|_| Err("autosave thread panicked".to_owned()));
                done.push((id, result));
            } else {
                i += 1;
            }
        }
        done
    }

    /// Forgets a closed document; returns its recovery key if it had one.
    pub fn forget(&mut self, doc: u64) -> Option<String> {
        self.marks.remove(&doc);
        self.keys.remove(&doc)
    }

    /// The document was saved by hand: nothing to recover until it changes.
    pub fn saved(&mut self, doc: u64) -> Option<String> {
        self.marks.remove(&doc);
        self.keys.get(&doc).cloned()
    }
}
