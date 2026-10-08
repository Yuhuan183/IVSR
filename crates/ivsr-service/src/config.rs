//! User configuration (`config.toml`), shared by the CLI and the desktop app.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ivsr_core::{AudioMode, FilterChain, FilterStage, ParamValue};
use ivsr_update::Channel;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Release repository; override at build time with `IVSR_UPDATE_REPOSITORY=owner/repo`.
pub const BUILTIN_UPDATE_REPOSITORY: &str = match option_env!("IVSR_UPDATE_REPOSITORY") {
    Some(repo) => repo,
    None => "Yuhuan183/IVSR",
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Engine used when none is requested.
    pub engine: String,
    /// Scratch space for intermediate files (video frames). Defaults to the system temp dir.
    pub work_dir: Option<PathBuf>,
    pub output: OutputConfig,
    pub video: VideoConfig,
    pub tools: ToolsConfig,
    /// Per-engine settings keyed by engine id.
    pub engines: BTreeMap<String, EngineConfig>,
    pub update: UpdateConfig,
    pub ui: UiConfig,
    pub models: ModelsConfig,
    pub history: HistoryConfig,
    pub filters: FiltersConfig,
}

/// Pre- and post-processing chains. Both are off until switched on, and a
/// chain without `steps` follows the built-in order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FiltersConfig {
    pub pre: FilterChain,
    pub post: FilterChain,
}

impl FiltersConfig {
    pub fn chain(&self, stage: FilterStage) -> &FilterChain {
        match stage {
            FilterStage::Pre => &self.pre,
            FilterStage::Post => &self.post,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// `auto` (follow the OS), `en` or `zh-TW`.
    pub language: String,
    /// Desktop app content scale (page zoom), 1.0 = 100%.
    pub scale: f64,
    /// Desktop app settings panel width, in CSS pixels at 100%.
    pub panel_width: u32,
    /// Desktop app viewer filter panel width, in CSS pixels at 100%.
    pub filter_panel_width: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelsConfig {
    /// Extra catalogue URLs (JSON in the same format as the built-in one).
    pub catalogs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// Record finished jobs for the desktop app's browse view.
    pub enabled: bool,
    /// Entries kept, newest first.
    pub limit: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { language: "auto".into(), scale: 1.0, panel_width: 340, filter_panel_width: 320 }
    }
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self { enabled: true, limit: 500 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Pick a free name (`photo_x4_1.png`).
    #[default]
    Rename,
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub scale: f64,
    /// Image format id, or `same` to keep the input format when it can be written.
    pub image_format: String,
    /// 1-100, for lossy formats.
    pub image_quality: u8,
    /// Appended to the file stem; `{scale}`, `{model}` and `{engine}` expand.
    pub suffix: String,
    /// Output directory; `None` writes next to each input.
    pub directory: Option<PathBuf>,
    pub conflict: ConflictPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    pub codec: String,
    /// Codec quality (CRF); `None` uses the codec default.
    pub quality: Option<u32>,
    pub preset: Option<String>,
    pub audio: AudioMode,
    /// Container id, or `same` to keep the input container when writable.
    pub container: String,
    /// Frames per engine invocation; bounds temporary disk usage.
    pub batch_frames: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    /// Explicit engine executable.
    pub path: Option<PathBuf>,
    pub models_dir: Option<PathBuf>,
    /// Default model id.
    pub model: Option<String>,
    /// Default engine parameters.
    pub params: BTreeMap<String, ParamValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Release source kind. Only `github` is implemented.
    pub provider: String,
    /// `owner/repo` for GitHub; empty disables update checks.
    pub repository: String,
    pub channel: Channel,
    pub auto_check: bool,
    pub interval_hours: u32,
    /// API endpoint override (GitHub Enterprise).
    pub api_base: Option<String>,
    /// Name of an environment variable holding an access token. Empty sends no token.
    pub token_env: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            engine: ivsr_engine_realesrgan::ENGINE_ID.into(),
            work_dir: None,
            output: OutputConfig::default(),
            video: VideoConfig::default(),
            tools: ToolsConfig::default(),
            engines: BTreeMap::new(),
            update: UpdateConfig::default(),
            ui: UiConfig::default(),
            models: ModelsConfig::default(),
            history: HistoryConfig::default(),
            filters: FiltersConfig::default(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            scale: 4.0,
            image_format: "same".into(),
            image_quality: 92,
            suffix: "_x{scale}".into(),
            directory: None,
            conflict: ConflictPolicy::Rename,
        }
    }
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            codec: "h264".into(),
            quality: None,
            preset: None,
            audio: AudioMode::Auto,
            container: "same".into(),
            batch_frames: 48,
        }
    }
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            provider: "github".into(),
            repository: BUILTIN_UPDATE_REPOSITORY.into(),
            channel: Channel::Stable,
            auto_check: true,
            interval_hours: 24,
            api_base: None,
            token_env: String::new(),
        }
    }
}

impl Config {
    /// Reads `path`; a missing file yields defaults.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Config(format!("{}: {e}", path.display()))),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::Config(format!("{}: {e}", parent.display())))?;
        }
        let staging = path.with_extension("toml.part");
        fs::write(&staging, text).map_err(|e| Error::Config(format!("{}: {e}", staging.display())))?;
        fs::rename(&staging, path).map_err(|e| Error::Config(format!("{}: {e}", path.display())))
    }

    pub fn engine_config(&self, id: &str) -> EngineConfig {
        self.engines.get(id).cloned().unwrap_or_default()
    }

    /// Reads a dotted key (`video.codec`, `engines.realesrgan.params.tile`).
    pub fn get(&self, key: &str) -> Result<Option<toml::Value>> {
        let tree = self.to_tree()?;
        Ok(walk(&tree, key).cloned())
    }

    /// Sets a dotted key. `raw` is parsed as a TOML value; when that does not
    /// fit the field, an integer is retried as a float and anything as a
    /// string (`video.preset 8`). The result must deserialize as a valid `Config`.
    pub fn set(&mut self, key: &str, raw: &str) -> Result<()> {
        let parsed = parse_value(raw);
        let mut candidates = vec![parsed.clone()];
        if let toml::Value::Integer(i) = parsed {
            candidates.push(toml::Value::Float(i as f64));
        }
        candidates.push(toml::Value::String(raw.to_string()));
        let mut first_err = None;
        for value in candidates {
            match self.edit(key, Some(value)) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        Err(first_err.expect("at least one candidate"))
    }

    /// Removes a dotted key, restoring its default.
    pub fn unset(&mut self, key: &str) -> Result<()> {
        self.edit(key, None)
    }

    fn to_tree(&self) -> Result<toml::Value> {
        toml::Value::try_from(self).map_err(|e| Error::Config(e.to_string()))
    }

    fn edit(&mut self, key: &str, value: Option<toml::Value>) -> Result<()> {
        let parts: Vec<&str> = key.split('.').filter(|p| !p.is_empty()).collect();
        let (last, parents) = parts.split_last().ok_or_else(|| Error::Config("empty key".into()))?;
        if !is_known_path(&parts) {
            return Err(Error::Config(format!("unknown setting `{key}`")));
        }
        let mut tree = self.to_tree()?;
        let mut table = tree.as_table_mut().expect("config serializes to a table");
        for part in parents {
            table = table
                .entry(part.to_string())
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .ok_or_else(|| Error::Config(format!("`{part}` is not a section")))?;
        }
        match value {
            Some(v) => {
                table.insert(last.to_string(), v);
            }
            None => {
                table.remove(*last);
            }
        }
        *self = tree.try_into().map_err(|e: toml::de::Error| Error::Config(format!("invalid value for `{key}`: {}", e.message())))?;
        Ok(())
    }
}

/// Guards against typos creating silently ignored keys.
fn is_known_path(parts: &[&str]) -> bool {
    let defaults = toml::Value::try_from(Config::default()).expect("default config serializes");
    match parts {
        // Free-form maps.
        ["engines", _engine, "params", _param] => true,
        ["engines", _engine, field] => matches!(*field, "path" | "models_dir" | "model"),
        ["filters", stage, field] => matches!(*stage, "pre" | "post") && matches!(*field, "enabled" | "steps"),
        [section, field] if matches!(*section, "output" | "video" | "tools" | "update" | "ui" | "models" | "history") => {
            let optional = matches!(
                (*section, *field),
                ("output", "directory") | ("video", "quality" | "preset") | ("tools", "ffmpeg" | "ffprobe") | ("update", "api_base")
            );
            optional || defaults.get(*section).and_then(|s| s.get(*field)).is_some()
        }
        [field] => *field == "work_dir" || defaults.get(*field).is_some_and(|v| !v.is_table()),
        _ => false,
    }
}

fn walk<'a>(tree: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    key.split('.').filter(|p| !p.is_empty()).try_fold(tree, |node, part| node.get(part))
}

fn parse_value(raw: &str) -> toml::Value {
    toml::from_str::<toml::Table>(&format!("v = {raw}"))
        .ok()
        .and_then(|mut t| t.remove("v"))
        .unwrap_or_else(|| toml::Value::String(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_defaults_and_save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg/config.toml");
        let mut cfg = Config::load(&path).unwrap();
        assert_eq!(cfg, Config::default());
        cfg.video.codec = "h265".into();
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), cfg);
    }

    #[test]
    fn dotted_set_parses_types_and_creates_engine_sections() {
        let mut cfg = Config::default();
        cfg.set("output.scale", "2").unwrap();
        cfg.set("video.preset", "8").unwrap();
        cfg.set("video.audio", "drop").unwrap();
        cfg.set("engines.realesrgan.params.tile", "256").unwrap();
        cfg.set("engines.realesrgan.model", "realesr-animevideov3").unwrap();
        cfg.set("work_dir", "/scratch").unwrap();
        cfg.set("ui.scale", "1.5").unwrap();
        cfg.set("ui.panel_width", "420").unwrap();
        assert_eq!(cfg.output.scale, 2.0);
        assert_eq!(cfg.video.preset.as_deref(), Some("8"));
        assert_eq!(cfg.video.audio, AudioMode::Drop);
        assert_eq!((cfg.ui.scale, cfg.ui.panel_width), (1.5, 420));
        let engine = cfg.engine_config("realesrgan");
        assert_eq!(engine.params.get("tile"), Some(&ParamValue::Int(256)));
        assert_eq!(engine.model.as_deref(), Some("realesr-animevideov3"));
        assert_eq!(cfg.get("engines.realesrgan.params.tile").unwrap(), Some(toml::Value::Integer(256)));
        cfg.unset("video.audio").unwrap();
        assert_eq!(cfg.video.audio, AudioMode::Auto);
    }

    #[test]
    fn filter_chains_are_set_by_dotted_keys_and_default_to_off() {
        let mut cfg = Config::default();
        assert!(!cfg.filters.post.enabled && cfg.filters.post.steps.is_none());
        cfg.set("filters.post.enabled", "true").unwrap();
        cfg.set("filters.pre.steps", r#"[{ id = "saturation", params = { amount = 1.2 } }]"#).unwrap();
        assert!(cfg.filters.post.enabled);
        let steps = cfg.filters.pre.steps.as_ref().unwrap();
        assert_eq!((steps[0].id.as_str(), steps[0].enabled), ("saturation", true));
        assert_eq!(steps[0].params.get("amount"), Some(&ParamValue::Float(1.2)));
        assert!(cfg.set("filters.mid.enabled", "true").is_err());
        cfg.unset("filters.pre.steps").unwrap();
        assert!(cfg.filters.pre.steps.is_none());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), cfg);
    }

    #[test]
    fn unknown_keys_and_bad_values_are_rejected_without_changes() {
        let mut cfg = Config::default();
        assert!(cfg.set("output.scael", "2.0").is_err());
        assert!(cfg.set("video.audio", "loud").is_err());
        assert!(cfg.set("engines.realesrgan.nope", "1").is_err());
        assert_eq!(cfg, Config::default());
    }
}
