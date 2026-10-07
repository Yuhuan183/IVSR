//! What hardware ivsr is running on.

use serde::Serialize;

use ivsr_core::process;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GpuInfo {
    pub index: Option<u32>,
    pub name: String,
    /// Memory available to the GPU, when known.
    pub memory_mb: Option<u64>,
    /// GPU shares system memory (Apple Silicon).
    pub unified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu: Option<String>,
    pub threads: usize,
    pub memory_mb: Option<u64>,
    pub gpus: Vec<GpuInfo>,
}

/// Collects system facts. `devices` come from the engine (names it can use).
pub fn collect(devices: &[ivsr_core::ComputeDevice]) -> SystemInfo {
    let memory_mb = total_memory_mb();
    let unified = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    let nvidia = if unified { Vec::new() } else { nvidia_smi() };
    let gpus = devices
        .iter()
        .map(|d| {
            let memory = if unified {
                memory_mb
            } else {
                nvidia.iter().find(|(name, _)| d.name.contains(name.as_str()) || name.contains(&d.name)).map(|(_, mb)| *mb)
            };
            GpuInfo { index: Some(d.index), name: d.name.clone(), memory_mb: memory, unified }
        })
        .collect();
    SystemInfo {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        cpu: cpu_name(),
        threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        memory_mb,
        gpus,
    }
}

/// `(name, MiB)` per NVIDIA GPU, if `nvidia-smi` is installed.
fn nvidia_smi() -> Vec<(String, u64)> {
    let Ok(out) = process::output(
        process::command("nvidia-smi").args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]),
        "nvidia-smi",
    ) else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|l| {
            let (name, mb) = l.rsplit_once(',')?;
            Some((name.trim().to_string(), mb.trim().parse().ok()?))
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn total_memory_mb() -> Option<u64> {
    let out = process::output(process::command("sysctl").args(["-n", "hw.memsize"]), "sysctl").ok()?;
    out.trim().parse::<u64>().ok().map(|b| b / 1024 / 1024)
}

#[cfg(target_os = "linux")]
fn total_memory_mb() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = info.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.split_whitespace().next()?.parse().ok()?;
    Some(kb / 1024)
}

#[cfg(windows)]
fn total_memory_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` is a properly sized, initialised MEMORYSTATUSEX.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then(|| status.ullTotalPhys / 1024 / 1024)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn total_memory_mb() -> Option<u64> {
    None
}

fn cpu_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        process::output(process::command("sysctl").args(["-n", "machdep.cpu.brand_string"]), "sysctl")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
    #[cfg(target_os = "linux")]
    {
        let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        info.lines().find_map(|l| l.strip_prefix("model name")).and_then(|l| l.split(':').nth(1)).map(|s| s.trim().into())
    }
    #[cfg(windows)]
    {
        std::env::var("PROCESSOR_IDENTIFIER").ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        None
    }
}
