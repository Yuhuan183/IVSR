//! Finished jobs, kept for the desktop app's browse view.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ivsr_core::{JobOutcome, JobSpec, MediaKind};
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix milliseconds at completion; unique enough as an id.
    pub id: u64,
    pub input: PathBuf,
    pub output: PathBuf,
    pub kind: MediaKind,
    pub engine: String,
    pub model: String,
    pub scale: f64,
    pub source_width: u32,
    pub source_height: u32,
    pub width: u32,
    pub height: u32,
    pub frames: Option<u64>,
    pub elapsed_ms: u64,
}

impl HistoryEntry {
    pub fn new(spec: &JobSpec, engine: &str, outcome: &JobOutcome, elapsed_ms: u64) -> Self {
        Self {
            id: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            input: spec.input.clone(),
            output: outcome.output.clone(),
            kind: spec.kind,
            engine: engine.into(),
            model: spec.settings.model.clone(),
            scale: spec.settings.scale,
            source_width: outcome.source_width,
            source_height: outcome.source_height,
            width: outcome.width,
            height: outcome.height,
            frames: outcome.frames,
            elapsed_ms,
        }
    }
}

pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let io = |e: std::io::Error| ivsr_core::Error::io_at("write", path, e);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let staging = path.with_extension(format!("{}.part", std::process::id()));
    fs::write(&staging, serde_json::to_vec_pretty(value).expect("serializable")).map_err(io)?;
    fs::rename(&staging, path).map_err(io)?;
    Ok(())
}

pub struct History {
    path: PathBuf,
    limit: usize,
}

impl History {
    pub fn new(path: PathBuf, limit: usize) -> Self {
        Self { path, limit: limit.max(1) }
    }

    /// Newest first.
    pub fn load(&self) -> Vec<HistoryEntry> {
        fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn record(&self, entry: HistoryEntry) -> Result<()> {
        let mut all = self.load();
        // Re-running into the same output replaces the old entry.
        all.retain(|e| e.output != entry.output);
        all.insert(0, entry);
        all.truncate(self.limit);
        write_json_atomic(&self.path, &all)
    }

    pub fn remove(&self, ids: &[u64]) -> Result<()> {
        let mut all = self.load();
        all.retain(|e| !ids.contains(&e.id));
        write_json_atomic(&self.path, &all)
    }

    pub fn clear(&self) -> Result<()> {
        write_json_atomic(&self.path, &Vec::<HistoryEntry>::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, output: &str) -> HistoryEntry {
        HistoryEntry {
            id,
            input: "in.png".into(),
            output: output.into(),
            kind: MediaKind::Image,
            engine: "e".into(),
            model: "m".into(),
            scale: 4.0,
            source_width: 1,
            source_height: 1,
            width: 4,
            height: 4,
            frames: None,
            elapsed_ms: 1,
        }
    }

    #[test]
    fn newest_first_capped_and_same_output_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let h = History::new(dir.path().join("history.json"), 2);
        h.record(entry(1, "a.png")).unwrap();
        h.record(entry(2, "b.png")).unwrap();
        h.record(entry(3, "a.png")).unwrap();
        let ids: Vec<u64> = h.load().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![3, 2]);
        h.record(entry(4, "c.png")).unwrap();
        assert_eq!(h.load().iter().map(|e| e.id).collect::<Vec<_>>(), vec![4, 3]);
        h.remove(&[3]).unwrap();
        assert_eq!(h.load().len(), 1);
    }
}
