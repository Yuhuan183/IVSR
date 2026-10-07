use std::process::ExitCode;

use console::style;
use ivsr_core::ToolStatus;
use ivsr_service::Service;
use serde_json::json;

use super::CmdResult;
use crate::tr;
use crate::ui::{self, Ui};

fn status_word(status: &ToolStatus) -> String {
    match status {
        ToolStatus::Ready { .. } => style(tr!("status.ready")).green().to_string(),
        ToolStatus::Missing { .. } => style(tr!("status.missing")).yellow().to_string(),
        ToolStatus::Broken { .. } => style(tr!("status.broken")).red().to_string(),
    }
}

pub fn run(service: &Service, ui: &Ui) -> CmdResult {
    let engine = service.config().engine.clone();
    let system = service.system_info(&engine)?;
    let engines: Vec<_> = service.engine_views().into_iter().map(|v| (v.info.id, v.status)).collect();
    let video = service.registry().video().status();
    if ui.json {
        ui.emit(&json!({ "system": system, "engines": engines, "video": video }));
        return Ok(ExitCode::SUCCESS);
    }
    let unknown = tr!("system.unknown");
    ui.heading(tr!("system.heading"));
    println!("  {}", tr!("system.os", os = system.os, arch = system.arch));
    println!("  {}", tr!("system.cpu", cpu = system.cpu.as_deref().unwrap_or(unknown), threads = system.threads));
    let memory = system.memory_mb.map(|m| ui::human_bytes(m * 1024 * 1024)).unwrap_or_else(|| unknown.to_string());
    println!("  {}", tr!("system.memory", memory = memory));
    println!();
    ui.heading(&tr!("system.gpus", engine = engine));
    if system.gpus.is_empty() {
        println!("  {}", style(tr!("system.no_gpus")).dim());
    }
    for gpu in &system.gpus {
        let memory = match (gpu.unified, gpu.memory_mb) {
            (true, _) => tr!("system.gpu_unified").to_string(),
            (false, Some(mb)) => tr!("system.gpu_memory", memory = ui::human_bytes(mb * 1024 * 1024)),
            (false, None) => tr!("system.gpu_unknown").to_string(),
        };
        let index = gpu.index.map(|i| format!("[{i}] ")).unwrap_or_default();
        println!("  {index}{}  {}", gpu.name, style(memory).dim());
    }
    println!();
    ui.heading(tr!("system.tools"));
    for (id, status) in &engines {
        println!("  {id:<12} {}", status_word(status));
    }
    println!("  {:<12} {}", "ffmpeg", status_word(&video));
    Ok(ExitCode::SUCCESS)
}
