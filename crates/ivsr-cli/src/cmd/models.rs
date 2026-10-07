use std::process::ExitCode;

use console::style;
use ivsr_core::{CancelToken, CostClass, ModelInfo, ModelManifest, Throughput};
use ivsr_service::advice::{self, Fit};
use ivsr_service::{BenchmarkRecord, ImportRequest, InstallProgress, ModelEntry, ModelStatus, Service};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;

use super::CmdResult;
use crate::cli::ModelsAction;
use crate::i18n::text;
use crate::tr;
use crate::ui::{self, Ui};

/// A 1920x1080 input frame, the unit for per-frame estimates.
const FRAME_MP: f64 = 1920.0 * 1080.0 / 1e6;

pub fn run(service: &Service, engine: Option<String>, action: Option<ModelsAction>, ui: &Ui) -> CmdResult {
    let engine = engine.unwrap_or_else(|| service.config().engine.clone());
    service.registry().engine(&engine)?;
    match action.unwrap_or(ModelsAction::List { refresh: false }) {
        ModelsAction::List { refresh } => list(service, &engine, refresh, ui),
        ModelsAction::Show { model } => show(service, &engine, &model, ui),
        ModelsAction::Install { models } => install(service, &engine, &models, ui),
        ModelsAction::Update { models, all } => update(service, &engine, models, all, ui),
        ModelsAction::Remove { model, yes } => remove(service, &engine, &model, yes, ui),
        ModelsAction::Import { id, param, bin, scale, name, description, license } => {
            let req = ImportRequest { id, name, description, scale, param, bin, license };
            import(service, &engine, &req, ui)
        }
        ModelsAction::Use { model } => use_model(service, &engine, &model, ui),
        ModelsAction::Bench { models, all, scale } => bench(service, &engine, models, all, scale, ui),
    }
}

fn status_label(status: ModelStatus) -> String {
    let (key, styled): (&str, fn(String) -> String) = match status {
        ModelStatus::Bundled => ("models.status.bundled", |s| style(s).dim().to_string()),
        ModelStatus::Installed => ("models.status.installed", |s| style(s).green().to_string()),
        ModelStatus::UpdateAvailable => ("models.status.update_available", |s| style(s).cyan().bold().to_string()),
        ModelStatus::Imported => ("models.status.imported", |s| style(s).magenta().to_string()),
        ModelStatus::Available => ("models.status.available", |s| style(s).yellow().to_string()),
    };
    styled(crate::i18n::t(key).to_string())
}

fn class_label(class: Option<CostClass>) -> &'static str {
    match class {
        Some(CostClass::Light) => tr!("class.light"),
        Some(CostClass::Medium) => tr!("class.medium"),
        Some(CostClass::Heavy) => tr!("class.heavy"),
        None => "",
    }
}

fn scales(list: &[u32]) -> String {
    list.iter().map(|s| format!("x{s}")).collect::<Vec<_>>().join(" ")
}

fn size_of(entry: &ModelEntry) -> u64 {
    entry.installed.as_ref().map(|i| i.size).unwrap_or_else(|| entry.manifest.as_ref().map_or(0, ModelManifest::download_size))
}

fn default_model(service: &Service, engine: &str) -> Option<String> {
    service
        .config()
        .engine_config(engine)
        .model
        .or_else(|| service.registry().engine(engine).ok()?.default_model())
}

fn list(service: &Service, engine: &str, refresh: bool, ui: &Ui) -> CmdResult {
    let overview = service.model_overview(engine, refresh)?;
    if ui.json {
        ui.emit(&overview);
        return Ok(ExitCode::SUCCESS);
    }
    for (url, reason) in &overview.catalog_errors {
        ui.warn(&tr!("models.catalog_error", url = url, reason = reason));
    }
    let default = default_model(service, engine);
    let rows: Vec<Vec<String>> = overview
        .entries
        .iter()
        .map(|e| {
            let (name, scale_list, license) = match (&e.installed, &e.manifest) {
                (Some(i), _) => (i.name.clone(), i.scales.clone(), i.license.clone()),
                (None, Some(m)) => (m.name.clone(), m.scales.clone(), m.license.clone()),
                (None, None) => (e.id.clone(), vec![], None),
            };
            let class = e.profile(&overview.architectures).map(|p| p.class);
            let marker = if default.as_deref() == Some(e.id.as_str()) { "*" } else { " " };
            vec![
                format!("{marker}{}", e.id),
                status_label(e.status),
                scales(&scale_list),
                class_label(class).to_string(),
                ui::human_bytes(size_of(e)),
                license.unwrap_or_default(),
                name,
            ]
        })
        .collect();
    ui::table(
        &[
            tr!("engines.col_model"),
            tr!("models.col_status"),
            tr!("engines.col_scales"),
            tr!("models.col_class"),
            tr!("models.col_size"),
            tr!("models.col_license"),
            tr!("engines.col_name"),
        ],
        &rows,
    );
    ui.info(&style(tr!("models.default_marker")).dim().to_string());
    let updates = overview.entries.iter().filter(|e| e.status == ModelStatus::UpdateAvailable).count();
    if updates > 0 {
        ui.hint(&tr!("models.hint_updates", count = updates));
    }
    if overview.entries.iter().any(|e| e.status == ModelStatus::Available) {
        ui.hint(tr!("models.hint_install"));
    }
    Ok(ExitCode::SUCCESS)
}

fn find_entry(service: &Service, engine: &str, id: &str) -> Result<ModelEntry, Box<dyn std::error::Error>> {
    service
        .model_overview(engine, false)?
        .entries
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| tr!("models.unknown", id = id).into())
}

fn seconds(s: f64) -> String {
    ui::human_duration((s * 1000.0) as u64)
}

fn show(service: &Service, engine: &str, id: &str, ui: &Ui) -> CmdResult {
    let overview = service.model_overview(engine, false)?;
    let entry = overview.entries.iter().find(|e| e.id == id).cloned().ok_or_else(|| tr!("models.unknown", id = id))?;
    let system = service.system_info(engine)?;
    let gpu = service.active_gpu(engine, &system);
    let local = service.benchmarks().into_iter().find(|b| {
        b.engine == engine && b.model == id && gpu.as_ref().is_none_or(|g| g.name == b.device)
    });
    let info = entry.installed.clone();
    let manifest = entry.manifest.clone();
    let catalog = service.registry().engine(engine)?.catalog().unwrap_or_default();
    let profile = entry.profile(&overview.architectures).cloned();
    let advice = advice::advise(profile.as_ref(), gpu.as_ref());
    if ui.json {
        ui.emit(&json!({
            "entry": entry, "hardware": profile, "advice": advice, "benchmark": local,
            "reference": catalog.reference, "system": system,
        }));
        return Ok(ExitCode::SUCCESS);
    }

    let pick = |i: Option<&ModelInfo>, m: Option<&ModelManifest>| -> (String, String, Vec<u32>) {
        match (i, m) {
            (Some(i), _) => (i.name.clone(), text(&i.description).to_string(), i.scales.clone()),
            (None, Some(m)) => (m.name.clone(), text(&m.description).to_string(), m.scales.clone()),
            _ => (id.to_string(), String::new(), vec![]),
        }
    };
    let (name, description, scale_list) = pick(info.as_ref(), manifest.as_ref());
    ui.heading(&format!("{name} ({id})  {}", status_label(entry.status)));
    if !description.is_empty() {
        println!("  {description}");
    }
    println!("  {}", tr!("models.scales", scales = scales(&scale_list)));
    let license = info.as_ref().and_then(|i| i.license.clone()).or_else(|| manifest.as_ref()?.license.clone());
    let author = info.as_ref().and_then(|i| i.author.clone()).or_else(|| manifest.as_ref()?.author.clone());
    let homepage = info.as_ref().and_then(|i| i.homepage.clone()).or_else(|| manifest.as_ref()?.homepage.clone());
    if let Some(l) = &license {
        println!("  {}", tr!("models.license", license = l));
    }
    if let Some(a) = &author {
        println!("  {}", tr!("models.author", author = a));
    }
    if let Some(h) = &homepage {
        println!("  {}", style(h).dim());
    }
    if let (Some(l), Some(a), Some(h)) = (&license, &author, &homepage) {
        if l.starts_with("CC-BY") {
            println!("  {}", style(tr!("models.attribution", author = a, homepage = h)).yellow());
        }
    }
    if let Some(v) = info.as_ref().and_then(|i| i.version.clone()).or_else(|| manifest.as_ref().map(|m| m.version.clone())) {
        if !v.is_empty() {
            println!("  {}", tr!("models.version", version = v));
        }
    }
    if let Some(src) = &entry.source {
        println!("  {}", tr!("models.source", source = src));
    }
    match &info {
        Some(i) => {
            println!("  {}", tr!("models.size_line", size = ui::human_bytes(i.size)));
            if let Some(p) = i.parameters {
                println!("  {}", tr!("models.parameters", count = format!("{:.1}M", p as f64 / 1e6)));
            }
        }
        None => {
            if let Some(m) = &manifest {
                println!("  {}", tr!("models.download_size", size = ui::human_bytes(m.download_size())));
            }
        }
    }

    if let Some(profile) = &profile {
        println!();
        ui.heading(tr!("models.hardware_heading"));
        println!("  {}", tr!("models.class_line", class = class_label(Some(profile.class))));
        println!("  {}", text(&profile.summary));
        let rows: Vec<Vec<String>> = profile
            .memory_by_tile
            .iter()
            .map(|t| {
                let tile = if t.tile == 0 { tr!("models.tile_auto").to_string() } else { t.tile.to_string() };
                vec![tile, ui::human_bytes(t.mb as u64 * 1024 * 1024)]
            })
            .collect();
        ui::table(&[tr!("models.col_tile"), tr!("models.col_memory")], &rows);
        println!("  {}", style(tr!("models.memory_note", device = catalog.reference.device)).dim());
    }

    println!();
    ui.heading(tr!("models.baseline_heading"));
    let baseline = info.as_ref().map(|i| i.baseline.clone()).filter(|b| !b.is_empty()).or_else(|| manifest.as_ref().map(|m| m.baseline.clone()));
    if let Some(points) = baseline.filter(|b| !b.is_empty()) {
        println!("  {}", tr!("models.reference", device = catalog.reference.device));
        print_points(&points, Throughput::fit(&points));
    }
    match &local {
        Some(b) => {
            let date = b.measured_at;
            println!("  {}", tr!("models.local", device = b.device, date = format_date(date)));
            print_points(&b.points, Some(b.throughput));
        }
        None if info.is_some() => println!("  {}", style(tr!("models.no_local", id = id)).dim()),
        None => {}
    }

    println!();
    ui.heading(tr!("models.advice_heading"));
    println!("  {}", advice_line(&advice, gpu.as_ref()));
    Ok(ExitCode::SUCCESS)
}

fn print_points(points: &[ivsr_core::BaselinePoint], fit: Option<Throughput>) {
    let rows: Vec<Vec<String>> =
        points.iter().map(|p| vec![format!("{}×{}", p.width, p.height), seconds(p.seconds)]).collect();
    ui::table(&[tr!("models.col_input"), tr!("models.col_time")], &rows);
    if let Some(t) = fit {
        println!(
            "  {}",
            style(tr!("models.per_mp", startup = format!("{:.2}", t.startup), rate = format!("{:.2}", t.per_megapixel))).dim()
        );
        println!("  {}", style(tr!("models.est_frame", time = seconds(t.estimate(FRAME_MP, 1)))).dim());
    }
}

fn format_date(unix: u64) -> String {
    // Days since epoch to a civil date (Howard Hinnant's algorithm).
    let days = (unix / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn mb(v: Option<u64>) -> String {
    v.map(|mb| ui::human_bytes(mb * 1024 * 1024)).unwrap_or_else(|| tr!("system.unknown").to_string())
}

fn advice_line(a: &advice::Advice, gpu: Option<&ivsr_service::GpuInfo>) -> String {
    let Some(gpu) = gpu else { return tr!("advice.no_gpu").to_string() };
    let needed = mb(a.needed_mb.map(u64::from));
    let available = mb(a.available_mb);
    let tile = a.suggested_tile.unwrap_or(0);
    match a.fit {
        Fit::Comfortable => tr!("advice.comfortable", gpu = gpu.name, available = available, needed = needed),
        Fit::Constrained => tr!("advice.constrained", gpu = gpu.name, available = available, needed = needed, tile = tile),
        Fit::Insufficient => tr!("advice.insufficient", gpu = gpu.name, available = available, tile = tile),
        Fit::Unknown => tr!("advice.unknown", needed = needed),
    }
}

fn progress_bar(ui: &Ui, total: u64) -> ProgressBar {
    if !ui.interactive || ui.quiet {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template("  {msg:<14} {bar:28.cyan/blue} {bytes:>9}/{total_bytes:<9} {bytes_per_sec}")
            .expect("valid template")
            .progress_chars("━━─"),
    );
    bar
}

fn install_one(service: &Service, engine: &str, id: &str, ui: &Ui) -> Result<ModelInfo, Box<dyn std::error::Error>> {
    let bar = progress_bar(ui, 0);
    bar.set_message(tr!("progress.resolving"));
    let info = service.install_model(
        engine,
        id,
        &mut |p| match p {
            InstallProgress::Downloading { received, total } => {
                bar.set_message(tr!("progress.downloading"));
                bar.set_length(total.unwrap_or(0));
                bar.set_position(received);
            }
            InstallProgress::Extracting => bar.set_message(tr!("progress.extracting")),
            _ => {}
        },
        &CancelToken::new(),
    );
    bar.finish_and_clear();
    Ok(info?)
}

fn install(service: &Service, engine: &str, ids: &[String], ui: &Ui) -> CmdResult {
    let mut installed = Vec::new();
    for id in ids {
        ui.info(&tr!("models.installing", id = id));
        let info = install_one(service, engine, id, ui)?;
        if !ui.json {
            eprintln!("{} {}", ui::ok_mark(), tr!("models.installed_ok", id = id, size = ui::human_bytes(info.size)));
        }
        installed.push(info);
    }
    if ui.json {
        ui.emit(&json!({ "event": "installed", "models": installed }));
    }
    Ok(ExitCode::SUCCESS)
}

fn update(service: &Service, engine: &str, ids: Vec<String>, all: bool, ui: &Ui) -> CmdResult {
    let overview = service.model_overview(engine, true)?;
    let targets: Vec<String> = if all || ids.is_empty() {
        overview.entries.iter().filter(|e| e.status == ModelStatus::UpdateAvailable).map(|e| e.id.clone()).collect()
    } else {
        ids
    };
    if targets.is_empty() {
        ui.info(tr!("models.no_updates"));
        return Ok(ExitCode::SUCCESS);
    }
    let mut updated = Vec::new();
    for id in targets {
        let entry = overview.entries.iter().find(|e| e.id == id).ok_or_else(|| tr!("models.unknown", id = id))?;
        if entry.status != ModelStatus::UpdateAvailable {
            ui.info(&tr!("models.up_to_date", id = id));
            continue;
        }
        install_one(service, engine, &id, ui)?;
        if !ui.json {
            eprintln!("{} {}", ui::ok_mark(), tr!("models.updated_ok", id = id));
        }
        updated.push(id);
    }
    if ui.json {
        ui.emit(&json!({ "event": "updated", "models": updated }));
    }
    Ok(ExitCode::SUCCESS)
}

fn remove(service: &Service, engine: &str, id: &str, yes: bool, ui: &Ui) -> CmdResult {
    let entry = find_entry(service, engine, id)?;
    let info = entry.installed.ok_or_else(|| tr!("models.not_installed", id = id))?;
    if !yes && !ui::confirm(ui, &tr!("models.confirm_remove", id = id, size = ui::human_bytes(info.size))) {
        if !ui.interactive {
            ui.hint(tr!("models.yes_hint"));
        }
        return Ok(ExitCode::FAILURE);
    }
    let cleared = service.remove_model(engine, id)?.is_some();
    if ui.json {
        ui.emit(&json!({ "event": "removed", "model": id, "default_cleared": cleared }));
    } else {
        eprintln!("{} {}", ui::ok_mark(), tr!("models.removed", id = id));
        if cleared {
            ui.info(&tr!("models.default_cleared", id = id));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn import(service: &Service, engine: &str, req: &ImportRequest, ui: &Ui) -> CmdResult {
    let info = service.import_model(engine, req)?;
    if ui.json {
        ui.emit(&json!({ "event": "imported", "model": info }));
    } else {
        eprintln!("{} {}", ui::ok_mark(), tr!("models.imported", id = info.id, scale = req.scale));
    }
    Ok(ExitCode::SUCCESS)
}

fn use_model(service: &Service, engine: &str, id: &str, ui: &Ui) -> CmdResult {
    let engine_ref = service.registry().engine(engine)?;
    if !engine_ref.models().iter().any(|m| m.id == id) {
        return Err(tr!("models.not_installed", id = id).into());
    }
    let mut config = service.config().clone();
    config.engine = engine.to_string();
    config.set(&format!("engines.{engine}.model"), id)?;
    config.save(service.config_file())?;
    if ui.json {
        ui.emit(&json!({ "event": "default_model", "engine": engine, "model": id }));
    } else {
        eprintln!("{} {}", ui::ok_mark(), tr!("models.use_ok", id = id));
    }
    Ok(ExitCode::SUCCESS)
}

fn bench(service: &Service, engine: &str, ids: Vec<String>, all: bool, scale: Option<u32>, ui: &Ui) -> CmdResult {
    let installed: Vec<String> = service.registry().engine(engine)?.models().into_iter().map(|m| m.id).collect();
    let targets = if all || ids.is_empty() && installed.len() == 1 { installed } else { ids };
    if targets.is_empty() {
        return Err(tr!("models.bench_none").into());
    }
    let mut results: Vec<BenchmarkRecord> = Vec::new();
    for id in &targets {
        let bar = if ui.interactive && !ui.quiet {
            let bar = ProgressBar::new(100);
            bar.set_style(ProgressStyle::with_template("  {msg} {bar:20.cyan/blue}").expect("valid").progress_chars("━━─"));
            bar.set_message(tr!("models.bench_running", id = id, scale = scale.map(|s| s.to_string()).unwrap_or_else(|| "·".into())));
            bar
        } else {
            ProgressBar::hidden()
        };
        let record = service.benchmark(engine, id, scale, &mut |f| bar.set_position((f * 100.0) as u64), &CancelToken::new());
        bar.finish_and_clear();
        let record = record?;
        if !ui.json {
            let t = |side: u32| record.points.iter().find(|p| p.width == side).map(|p| seconds(p.seconds)).unwrap_or_default();
            eprintln!(
                "{} {}",
                ui::ok_mark(),
                tr!(
                    "models.bench_result",
                    id = id,
                    scale = record.scale,
                    device = record.device,
                    t256 = t(256),
                    t512 = t(512),
                    frame = seconds(record.throughput.estimate(FRAME_MP, 1))
                )
            );
        }
        results.push(record);
    }
    if ui.json {
        ui.emit(&json!({ "event": "benchmarks", "results": results }));
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_format_as_iso() {
        assert_eq!(super::format_date(0), "1970-01-01");
        assert_eq!(super::format_date(1_791_331_200), "2026-10-07");
    }
}
