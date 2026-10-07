//! Where ivsr keeps configuration, managed engines, caches and state.

use std::path::{Path, PathBuf};

use serde::Serialize;

pub const APP_DIR: &str = "ivsr";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppPaths {
    pub config_file: PathBuf,
    /// Managed engine installs live in `engines/<id>/`.
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    /// Platform directories, or everything under `$IVSR_HOME` when set
    /// (portable installs, tests).
    pub fn discover() -> Self {
        if let Some(home) = std::env::var_os("IVSR_HOME").filter(|v| !v.is_empty()) {
            return Self::rooted(Path::new(&home));
        }
        let base = |dir: Option<PathBuf>| dir.unwrap_or_else(std::env::temp_dir).join(APP_DIR);
        Self {
            config_file: base(dirs::config_dir()).join("config.toml"),
            data_dir: base(dirs::data_local_dir()),
            cache_dir: base(dirs::cache_dir()),
        }
    }

    pub fn rooted(root: &Path) -> Self {
        Self { config_file: root.join("config.toml"), data_dir: root.join("data"), cache_dir: root.join("cache") }
    }

    pub fn engine_dir(&self, id: &str) -> PathBuf {
        self.data_dir.join("engines").join(id)
    }

    /// Managed models for engine `id`, one subdirectory per model.
    pub fn model_store(&self, id: &str) -> PathBuf {
        self.data_dir.join("models").join(id)
    }

    pub fn catalog_cache(&self) -> PathBuf {
        self.cache_dir.join("catalogs")
    }

    pub fn history_file(&self) -> PathBuf {
        self.data_dir.join("history.json")
    }

    pub fn benchmarks_file(&self) -> PathBuf {
        self.data_dir.join("benchmarks.json")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.cache_dir.join("downloads")
    }

    pub fn update_state(&self) -> PathBuf {
        self.data_dir.join("update-state.json")
    }

    pub fn default_work_dir(&self) -> PathBuf {
        std::env::temp_dir().join(APP_DIR)
    }
}
