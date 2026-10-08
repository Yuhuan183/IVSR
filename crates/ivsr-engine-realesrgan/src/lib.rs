//! `Engine` implementation backed by the `realesrgan-ncnn-vulkan` binary.

mod models;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ivsr_core::params::EnumOption;
use ivsr_core::process;
use ivsr_core::{
    Catalog, ComputeDevice, Distribution, Engine, EngineCaps, EngineInfo, Error, LogLevel, ModelInfo, ModelManifest,
    ParamKind, ParamSpec, ParamValue, Result, TaskContext, TaskMode, Text, ToolStatus, UpscaleTask,
};

use models::Located;

pub const ENGINE_ID: &str = "realesrgan";
const TOOL: &str = "realesrgan-ncnn-vulkan";
const BINARY: &str = if cfg!(windows) { "realesrgan-ncnn-vulkan.exe" } else { "realesrgan-ncnn-vulkan" };
/// First line of every ncnn `.param` file.
pub const NCNN_PARAM_MAGIC: &str = "7767517";

#[derive(Debug, Clone, Default)]
pub struct RealEsrganConfig {
    /// Explicit binary path; overrides discovery.
    pub binary: Option<PathBuf>,
    /// Bundled model directory; defaults to `models/` next to the binary.
    pub models_dir: Option<PathBuf>,
    /// Managed install location searched before `PATH`.
    pub install_dir: Option<PathBuf>,
    /// Directory of managed models (`<store>/<id>/models/<id>.*`).
    pub store_dir: Option<PathBuf>,
}

pub struct RealEsrgan {
    config: RealEsrganConfig,
    devices: OnceLock<Vec<ComputeDevice>>,
}

impl RealEsrgan {
    pub fn new(config: RealEsrganConfig) -> Self {
        Self { config, devices: OnceLock::new() }
    }

    fn locate(&self) -> std::result::Result<PathBuf, String> {
        if let Some(path) = &self.config.binary {
            return if path.is_file() {
                Ok(path.clone())
            } else {
                Err(format!("configured binary {} does not exist", path.display()))
            };
        }
        if let Some(dir) = &self.config.install_dir {
            if let Some(found) = find_in(dir) {
                return Ok(found);
            }
        }
        which::which(BINARY).map_err(|_| {
            format!("{TOOL} not found; run `ivsr engines install {ENGINE_ID}` or set engines.{ENGINE_ID}.path")
        })
    }

    fn bundled_dir(&self, binary: &Path) -> PathBuf {
        self.config
            .models_dir
            .clone()
            .unwrap_or_else(|| binary.parent().unwrap_or(Path::new(".")).join("models"))
    }

    /// Every usable model: bundled first, then managed.
    fn located(&self) -> Vec<Located> {
        let mut all = match self.locate() {
            Ok(binary) => {
                let dir = self.bundled_dir(&binary);
                if check_models_dir(&dir).is_ok() { models::scan_bundled(&dir) } else { Vec::new() }
            }
            Err(_) => Vec::new(),
        };
        if let Some(store) = &self.config.store_dir {
            for managed in models::scan_store(store) {
                if !all.iter().any(|l| l.info.id == managed.info.id) {
                    all.push(managed);
                }
            }
        }
        all
    }
}

/// Finds the binary directly in `dir` or one level below (archives often nest).
fn find_in(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join(BINARY);
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(dir).ok()?.flatten().map(|e| e.path().join(BINARY)).find(|p| p.is_file())
}

/// The binary rejects model directories whose path lacks `models`.
fn check_models_dir(dir: &Path) -> std::result::Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("model directory {} does not exist", dir.display()));
    }
    if !dir.to_string_lossy().contains("models") {
        return Err(format!("model directory path must contain `models` ({TOOL} restriction): {}", dir.display()));
    }
    Ok(())
}

/// Parses `[0 Apple M4 Pro]  queueC=0[1] ...` device lines.
fn parse_device(line: &str) -> Option<ComputeDevice> {
    let inner = line.strip_prefix('[')?.split(']').next()?;
    let (index, name) = inner.split_once(' ')?;
    Some(ComputeDevice { index: index.parse().ok()?, name: name.trim().to_string() })
}

/// An 8x8 PNG, enough to make the binary initialise its GPUs.
const PROBE_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41,
    220, 0, 0, 0, 21, 73, 68, 65, 84, 120, 218, 99, 140, 170, 152, 198, 128, 13, 48, 49, 224, 0, 131, 83, 2, 0, 29, 75,
    1, 120, 119, 176, 0, 144, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn text(en: &str, zh: &str) -> Text {
    Text::en(en).with("zh-TW", zh)
}

impl Engine for RealEsrgan {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            id: ENGINE_ID.into(),
            name: "Real-ESRGAN".into(),
            description: text(
                "Practical blind super-resolution (ncnn + Vulkan, runs on most GPUs).",
                "實用的盲超解析度 (ncnn + Vulkan, 多數 GPU 皆可執行).",
            ),
            homepage: Some("https://github.com/xinntao/Real-ESRGAN".into()),
        }
    }

    fn status(&self) -> ToolStatus {
        let binary = match self.locate() {
            Ok(b) => b,
            Err(hint) => return ToolStatus::Missing { hint },
        };
        let bundled = self.bundled_dir(&binary);
        let store_has_models = self.config.store_dir.as_ref().is_some_and(|s| !models::scan_store(s).is_empty());
        if let Err(reason) = check_models_dir(&bundled) {
            if !store_has_models {
                return ToolStatus::Broken { reason };
            }
        }
        if self.located().is_empty() {
            return ToolStatus::Broken { reason: format!("no model weights in {}", bundled.display()) };
        }
        ToolStatus::Ready { location: binary, version: None }
    }

    fn models(&self) -> Vec<ModelInfo> {
        self.located().into_iter().map(|l| l.info).collect()
    }

    fn default_model(&self) -> Option<String> {
        let models = self.models();
        models.iter().find(|m| m.id == "realesrgan-x4plus").or(models.first()).map(|m| m.id.clone())
    }

    fn params(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec {
                key: "tile".into(),
                label: text("Tile size", "Tile 大小"),
                description: text(
                    "Split the image into tiles of this size (0 = automatic, otherwise >= 32). \
                     Smaller tiles use less GPU memory.",
                    "將圖片切成此大小的區塊處理 (0 = 自動, 否則需 >= 32). 區塊越小越省 GPU 記憶體.",
                ),
                kind: ParamKind::Int { min: Some(0), max: Some(4096) },
                default: ParamValue::Int(0),
                advanced: false,
            },
            ParamSpec {
                key: "tta".into(),
                label: text("TTA mode", "TTA 模式"),
                description: text(
                    "Test-time augmentation: slightly cleaner results, about 8x slower.",
                    "測試時增強: 結果稍微乾淨, 但約慢 8 倍.",
                ),
                kind: ParamKind::Bool,
                default: ParamValue::Bool(false),
                advanced: false,
            },
            ParamSpec {
                key: "gpu".into(),
                label: text("GPU", "GPU"),
                description: text(
                    "Vulkan device index, a comma list for multi-GPU (0,1), or auto.",
                    "Vulkan 裝置編號, 多 GPU 用逗號分隔 (0,1), 或 auto.",
                ),
                kind: ParamKind::Text,
                default: ParamValue::Text("auto".into()),
                advanced: true,
            },
            ParamSpec {
                key: "threads".into(),
                label: text("Threads", "執行緒"),
                description: text("Thread count for load:process:save.", "載入:處理:儲存 的執行緒數量."),
                kind: ParamKind::Enum {
                    options: ["1:2:2", "1:1:1", "2:2:2", "2:4:4", "4:4:4"]
                        .iter()
                        .map(|v| EnumOption { value: (*v).into(), label: (*v).into() })
                        .collect(),
                },
                default: ParamValue::Text("1:2:2".into()),
                advanced: true,
            },
        ]
    }

    fn caps(&self) -> EngineCaps {
        EngineCaps {
            input_formats: vec!["png".into(), "jpg".into(), "webp".into()],
            output_format: "png".into(),
            batch: true,
        }
    }

    fn distribution(&self) -> Option<Distribution> {
        Some(Distribution {
            provider: "github".into(),
            locator: "xinntao/Real-ESRGAN".into(),
            tag: Some("v0.2.5.0".into()),
            assets: vec![
                ("macos".into(), "-macos.zip".into()),
                ("linux".into(), "-ubuntu.zip".into()),
                ("windows".into(), "-windows.zip".into()),
            ],
            executable: BINARY.into(),
        })
    }

    fn install_dir(&self) -> Option<PathBuf> {
        self.config.install_dir.clone()
    }

    fn catalog(&self) -> Option<Catalog> {
        Some(models::catalog().clone())
    }

    fn model_store(&self) -> Option<PathBuf> {
        self.config.store_dir.clone()
    }

    /// Managed models hold one scale each, stored as `models/<id>.{param,bin}`
    /// so the leaf directory satisfies the binary's `models` path rule.
    fn model_layout(&self, manifest: &ModelManifest) -> Result<Vec<(String, PathBuf)>> {
        let invalid = |reason: String| Error::Invalid(format!("model `{}`: {reason}", manifest.id));
        if manifest.engine != ENGINE_ID {
            return Err(invalid(format!("is for engine `{}`", manifest.engine)));
        }
        if !ivsr_core::model::valid_model_id(&manifest.id) || manifest.id == "realesr-animevideov3" {
            return Err(invalid("invalid id (use letters, digits, `-`, `_`, `.`)".into()));
        }
        match manifest.scales.as_slice() {
            [s] if (1..=8).contains(s) => {}
            _ => return Err(invalid("managed models must declare exactly one scale between 1 and 8".into())),
        }
        let roles: Vec<&str> = manifest.files.iter().map(|f| f.role.as_str()).collect();
        if roles.len() != 2 || !roles.contains(&"param") || !roles.contains(&"bin") {
            return Err(invalid("needs exactly one `param` and one `bin` file".into()));
        }
        Ok(["param", "bin"]
            .iter()
            .map(|role| (role.to_string(), PathBuf::from("models").join(format!("{}.{role}", manifest.id))))
            .collect())
    }

    fn devices(&self) -> Vec<ComputeDevice> {
        self.devices
            .get_or_init(|| {
                let (Ok(binary), Some(model)) = (self.locate(), self.located().into_iter().min_by_key(|l| l.info.size))
                else {
                    return Vec::new();
                };
                let Ok(dir) = tempfile::tempdir() else { return Vec::new() };
                let input = dir.path().join("probe.png");
                if fs::write(&input, PROBE_PNG).is_err() {
                    return Vec::new();
                }
                let scale = model.info.scales[0];
                let mut cmd = process::command(&binary);
                cmd.arg("-i").arg(&input).arg("-o").arg(dir.path().join("out.png"));
                cmd.arg("-m").arg(&model.dir).arg("-n").arg(&model.name).arg("-s").arg(scale.to_string());
                let mut devices: Vec<ComputeDevice> = Vec::new();
                let _ = process::run(
                    &mut cmd,
                    TOOL,
                    &ivsr_core::CancelToken::new(),
                    |line| {
                        if let Some(d) = parse_device(line).filter(|d| !devices.iter().any(|e| e.index == d.index)) {
                            devices.push(d);
                        }
                    },
                    || {},
                );
                devices
            })
            .clone()
    }

    fn upscale(&self, task: &UpscaleTask<'_>, ctx: &TaskContext<'_>) -> Result<()> {
        let unavailable = |reason: String| Error::EngineUnavailable { engine: ENGINE_ID.into(), reason };
        let binary = self.locate().map_err(unavailable)?;

        let not_found = || Error::ModelNotFound { engine: ENGINE_ID.into(), model: task.model.into() };
        let model = self.located().into_iter().find(|l| l.info.id == task.model).ok_or_else(not_found)?;
        check_models_dir(&model.dir).map_err(unavailable)?;
        if !model.info.scales.contains(&task.scale) || !model.has_weights(task.scale) {
            return Err(Error::Invalid(format!(
                "model `{}` has no weights for x{} in {}",
                model.info.id,
                task.scale,
                model.dir.display()
            )));
        }

        // The tool runs in its own directory (it finds bundled models there),
        // so every path handed to it must be absolute.
        let absolute = |p: &Path| std::path::absolute(p).map_err(|e| Error::io_at("resolve", p, e));
        let mut cmd = process::command(&binary);
        cmd.arg("-i").arg(absolute(task.input)?).arg("-o").arg(absolute(task.output)?);
        cmd.arg("-m").arg(absolute(&model.dir)?).arg("-n").arg(&model.name).arg("-s").arg(task.scale.to_string());
        cmd.arg("-f").arg("png");
        apply_params(&mut cmd, task)?;
        if let Some(dir) = binary.parent() {
            cmd.current_dir(dir);
        }

        let progress = ctx.progress;
        let finished = match task.mode {
            TaskMode::File => process::run(
                &mut cmd,
                TOOL,
                ctx.cancel,
                |line| {
                    if let Some(p) = parse_percent(line) {
                        progress(p / 100.0);
                    } else {
                        (ctx.log)(LogLevel::Debug, line);
                    }
                },
                || {},
            )?,
            TaskMode::Directory { count } => {
                fs::create_dir_all(task.output).map_err(|e| Error::io_at("create directory", task.output, e))?;
                let total = count.max(1) as f64;
                // Percent lines interleave across worker threads; finished files are reliable.
                process::run(
                    &mut cmd,
                    TOOL,
                    ctx.cancel,
                    |line| {
                        if parse_percent(line).is_none() {
                            (ctx.log)(LogLevel::Debug, line);
                        }
                    },
                    || progress(count_files(task.output) as f64 / total),
                )?
            }
        };
        finished.into_result(TOOL)?;
        if task.mode == TaskMode::File && !task.output.is_file() {
            return Err(Error::tool(TOOL, "finished without writing an output file"));
        }
        progress(1.0);
        Ok(())
    }
}

fn apply_params(cmd: &mut std::process::Command, task: &UpscaleTask<'_>) -> Result<()> {
    let invalid = |key: &str, reason: &str| Error::InvalidParam { key: key.into(), reason: reason.into() };
    if let Some(tile) = task.params.get("tile").and_then(ParamValue::as_i64) {
        match tile {
            0 => {}
            t if t >= 32 => {
                cmd.arg("-t").arg(t.to_string());
            }
            _ => return Err(invalid("tile", "must be 0 (auto) or at least 32")),
        }
    }
    if task.params.get("tta").and_then(ParamValue::as_bool) == Some(true) {
        cmd.arg("-x");
    }
    if let Some(gpu) = task.params.get("gpu").and_then(ParamValue::as_str) {
        let gpu = gpu.trim();
        if !gpu.is_empty() && gpu != "auto" {
            if !gpu.split(',').all(|g| g.trim().parse::<u32>().is_ok()) {
                return Err(invalid("gpu", "expected `auto`, a device index, or a comma-separated list"));
            }
            cmd.arg("-g").arg(gpu);
        }
    }
    if let Some(threads) = task.params.get("threads").and_then(ParamValue::as_str) {
        cmd.arg("-j").arg(threads);
    }
    Ok(())
}

fn parse_percent(line: &str) -> Option<f64> {
    line.trim().strip_suffix('%')?.trim().parse().ok()
}

fn count_files(dir: &Path) -> usize {
    fs::read_dir(dir).map(|d| d.flatten().filter(|e| e.path().is_file()).count()).unwrap_or(0)
}

#[cfg(test)]
mod tests;
