//! Remembering when updates were last checked, for automatic checks.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckState {
    /// Unix seconds of the last successful check.
    pub last_check: Option<u64>,
    /// Newest version seen at that check, if newer than the running one.
    pub available: Option<String>,
    /// A version the user chose to skip.
    pub skipped: Option<String>,
}

impl CheckState {
    pub fn is_due(&self, interval: Duration, now: SystemTime) -> bool {
        let now = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        self.last_check.is_none_or(|last| now.saturating_sub(last) >= interval.as_secs())
    }

    pub fn record(&mut self, available: Option<String>, now: SystemTime) {
        self.last_check = Some(now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs());
        self.available = available;
    }
}

/// JSON file persistence for `CheckState`. Unreadable files read as empty.
pub struct StateStore {
    path: PathBuf,
}

impl StateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> CheckState {
        fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, state: &CheckState) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_vec_pretty(state)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_is_due_after_interval_and_state_round_trips() {
        let day = Duration::from_secs(86_400);
        let t0 = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut state = CheckState::default();
        assert!(state.is_due(day, t0));
        state.record(Some("0.2.0".into()), t0);
        assert!(!state.is_due(day, t0 + Duration::from_secs(3600)));
        assert!(state.is_due(day, t0 + day));

        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::new(dir.path().join("nested/update.json"));
        assert_eq!(store.load(), CheckState::default());
        store.save(&state).unwrap();
        assert_eq!(store.load(), state);
    }
}
