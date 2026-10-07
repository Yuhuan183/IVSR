//! Measuring model throughput on this machine.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ivsr_core::{
    BaselinePoint, CancelToken, ComputeDevice, Engine, ParamValues, TaskContext, TaskMode, Throughput, UpscaleTask,
};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Input sizes timed for the two-point fit; a tiny run first warms caches.
const WARMUP_SIDE: u32 = 64;
const SIDES: [u32; 2] = [256, 512];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkRecord {
    pub engine: String,
    pub model: String,
    pub scale: u32,
    pub device: String,
    pub points: Vec<BaselinePoint>,
    pub throughput: Throughput,
    /// Unix seconds.
    pub measured_at: u64,
}

pub(crate) struct BenchStore {
    path: PathBuf,
}

impl BenchStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Vec<BenchmarkRecord> {
        fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Replaces any record for the same engine, model, scale and device.
    pub fn save(&self, record: &BenchmarkRecord) -> Result<()> {
        let mut all = self.load();
        all.retain(|r| {
            !(r.engine == record.engine && r.model == record.model && r.scale == record.scale && r.device == record.device)
        });
        all.push(record.clone());
        crate::history::write_json_atomic(&self.path, &all)
    }
}

/// The device a run with `params` will use.
pub(crate) fn device_for(devices: &[ComputeDevice], params: &ParamValues) -> String {
    let wanted = params.get("gpu").and_then(|v| v.as_str()).and_then(|g| g.split(',').next()?.trim().parse::<u32>().ok());
    wanted
        .and_then(|i| devices.iter().find(|d| d.index == i))
        .or(devices.first())
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "unknown device".into())
}

pub(crate) fn run(
    engine: &dyn Engine,
    model: &str,
    scale: u32,
    params: &ParamValues,
    work_root: &Path,
    progress: &mut dyn FnMut(f64),
    cancel: &CancelToken,
) -> Result<BenchmarkRecord> {
    let info = engine
        .models()
        .into_iter()
        .find(|m| m.id == model)
        .ok_or_else(|| Error::Input(format!("model `{model}` is not installed")))?;
    if !info.scales.contains(&scale) {
        return Err(Error::Input(format!("model `{model}` does not run at x{scale}")));
    }
    let device = device_for(&engine.devices(), params);
    fs::create_dir_all(work_root).map_err(|e| ivsr_core::Error::io_at("create", work_root, e))?;
    let dir = tempfile::Builder::new()
        .prefix("ivsr-bench-")
        .tempdir_in(work_root)
        .map_err(|e| ivsr_core::Error::io_at("create", work_root, e))?;

    let steps = 1 + SIDES.len();
    let mut points = Vec::new();
    for (step, side) in std::iter::once(WARMUP_SIDE).chain(SIDES).enumerate() {
        cancel.check()?;
        let input = dir.path().join(format!("in{side}.png"));
        let output = dir.path().join(format!("out{side}.png"));
        ivsr_media::testpattern::write_png(&input, side, side)?;
        let base = step as f64 / steps as f64;
        let report = |_: f64| {};
        let log = |_: ivsr_core::LogLevel, _: &str| {};
        let task = UpscaleTask { input: &input, output: &output, mode: TaskMode::File, model, scale, params };
        let started = Instant::now();
        engine.upscale(&task, &TaskContext { cancel, progress: &report, log: &log })?;
        let seconds = started.elapsed().as_secs_f64();
        if side != WARMUP_SIDE {
            points.push(BaselinePoint { width: side, height: side, seconds });
        }
        progress(base + 1.0 / steps as f64);
    }
    let throughput = Throughput::fit(&points).ok_or_else(|| Error::Input("benchmark produced no usable timings".into()))?;
    Ok(BenchmarkRecord {
        engine: engine.info().id,
        model: model.into(),
        scale,
        device,
        points,
        throughput,
        measured_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
    })
}
