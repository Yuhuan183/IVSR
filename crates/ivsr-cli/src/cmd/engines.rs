use std::process::ExitCode;

use console::style;
use ivsr_core::{CancelToken, ParamKind, ToolStatus};
use ivsr_service::{EngineView, InstallProgress, Service};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;

use super::CmdResult;
use crate::cli::EnginesAction;
use crate::i18n::text;
use crate::tr;
use crate::ui::{self, Ui};

pub fn run(service: &Service, action: Option<EnginesAction>, ui: &Ui) -> CmdResult {
    match action.unwrap_or(EnginesAction::List) {
        EnginesAction::List => list(service, ui),
        EnginesAction::Show { engine } => show(service, &engine, ui),
        EnginesAction::Install { engine, force } => install(service, &engine, force, ui),
    }
}

fn status_cell(status: &ToolStatus) -> String {
    match status {
        ToolStatus::Ready { .. } => format!("{} {}", style("●").green(), tr!("status.ready")),
        ToolStatus::Missing { .. } => format!("{} {}", style("○").yellow(), tr!("status.missing")),
        ToolStatus::Broken { .. } => format!("{} {}", style("●").red(), tr!("status.broken")),
    }
}

fn list(service: &Service, ui: &Ui) -> CmdResult {
    let views = service.engine_views();
    if ui.json {
        ui.emit(&views);
        return Ok(ExitCode::SUCCESS);
    }
    let default = &service.config().engine;
    let rows: Vec<Vec<String>> = views
        .iter()
        .map(|v| {
            let marker = if &v.info.id == default { "*" } else { " " };
            let detail = match &v.status {
                ToolStatus::Ready { location, .. } => location.display().to_string(),
                other => other.problem().unwrap_or_default().to_string(),
            };
            vec![format!("{marker}{}", v.info.id), v.info.name.clone(), status_cell(&v.status), v.models.len().to_string(), detail]
        })
        .collect();
    ui::table(
        &[
            tr!("engines.col_id"),
            tr!("engines.col_name"),
            tr!("engines.col_status"),
            tr!("engines.col_models"),
            tr!("engines.col_location"),
        ],
        &rows,
    );
    if views.iter().any(|v| !v.status.is_ready() && v.installable) {
        ui.hint(tr!("engines.install_hint"));
    }
    Ok(ExitCode::SUCCESS)
}

fn find<'a>(views: &'a [EngineView], id: &str) -> Result<&'a EngineView, Box<dyn std::error::Error>> {
    views.iter().find(|v| v.info.id == id).ok_or_else(|| tr!("engines.unknown", id = id).into())
}

fn show(service: &Service, id: &str, ui: &Ui) -> CmdResult {
    let views = service.engine_views();
    let view = find(&views, id)?;
    if ui.json {
        ui.emit(view);
        return Ok(ExitCode::SUCCESS);
    }
    ui.heading(&format!("{} ({})", view.info.name, view.info.id));
    println!("  {}", text(&view.info.description));
    if let Some(home) = &view.info.homepage {
        println!("  {}", style(home).dim());
    }
    println!("  {}", tr!("engines.status_line", status = status_cell(&view.status)));
    match &view.status {
        ToolStatus::Ready { location, .. } => println!("  {}", tr!("engines.binary", path = location.display())),
        other => println!("  {}", style(other.problem().unwrap_or_default()).yellow()),
    }
    if let Some(m) = &view.installed {
        println!("  {}", tr!("engines.installed", release = m.release, asset = m.asset));
    }

    println!();
    ui.heading(tr!("engines.models_heading"));
    if view.models.is_empty() {
        println!("  {}", style(tr!("engines.no_models")).dim());
    } else {
        let configured = service.config().engine_config(id).model;
        let default = configured.or(view.default_model.clone());
        let rows: Vec<Vec<String>> = view
            .models
            .iter()
            .map(|m| {
                let marker = if Some(&m.id) == default.as_ref() { "*" } else { " " };
                let scales: Vec<String> = m.scales.iter().map(|s| format!("x{s}")).collect();
                vec![format!("{marker}{}", m.id), scales.join(" "), m.tags.join(", "), text(&m.description).to_string()]
            })
            .collect();
        ui::table(
            &[tr!("engines.col_model"), tr!("engines.col_scales"), tr!("engines.col_tags"), tr!("engines.col_description")],
            &rows,
        );
    }

    println!();
    ui.heading(tr!("engines.params_heading"));
    let rows: Vec<Vec<String>> = view
        .params
        .iter()
        .map(|p| vec![p.key.clone(), param_kind(&p.kind), p.default.to_string(), text(&p.description).to_string()])
        .collect();
    ui::table(
        &[tr!("engines.col_key"), tr!("engines.col_type"), tr!("engines.col_default"), tr!("engines.col_description")],
        &rows,
    );
    Ok(ExitCode::SUCCESS)
}

/// `int 0..4096`, `bool`, `a|b|c`: a parameter's type for help tables.
pub(crate) fn param_kind(kind: &ParamKind) -> String {
    let range = |min: String, max: String| format!("{min}..{max}");
    match kind {
        ParamKind::Bool => "bool".to_string(),
        ParamKind::Int { min, max } => format!(
            "int {}",
            range(min.map(|v| v.to_string()).unwrap_or_default(), max.map(|v| v.to_string()).unwrap_or_default())
        ),
        ParamKind::Float { min, max } => format!(
            "float {}",
            range(min.map(|v| v.to_string()).unwrap_or_default(), max.map(|v| v.to_string()).unwrap_or_default())
        ),
        ParamKind::Enum { options } => options.iter().map(|o| o.value.as_str()).collect::<Vec<_>>().join("|"),
        ParamKind::Text => "text".to_string(),
    }
}

fn install(service: &Service, id: &str, force: bool, ui: &Ui) -> CmdResult {
    let views = service.engine_views();
    let view = find(&views, id)?;
    if view.status.is_ready() && !force {
        ui.info(&tr!("engines.already", name = view.info.name, status = status_cell(&view.status)));
        ui.hint(tr!("engines.force_hint"));
        return Ok(ExitCode::SUCCESS);
    }
    if !view.installable {
        return Err(tr!("engines.not_installable", name = view.info.name).into());
    }

    let bar = if ui.interactive && !ui.quiet {
        let bar = ProgressBar::new(0);
        bar.set_style(
            ProgressStyle::with_template("  {msg:<12} {bar:28.cyan/blue} {bytes:>9}/{total_bytes:<9} {bytes_per_sec}")
                .expect("valid template")
                .progress_chars("━━─"),
        );
        bar
    } else {
        ProgressBar::hidden()
    };
    let json = ui.json;
    let mut last_emit = std::time::Instant::now();
    let report = service.install_engine(
        id,
        &mut |p| {
            match &p {
                InstallProgress::Resolving => bar.set_message(tr!("progress.resolving")),
                InstallProgress::Downloading { received, total } => {
                    bar.set_message(tr!("progress.downloading"));
                    if let Some(t) = total {
                        bar.set_length(*t);
                    }
                    bar.set_position(*received);
                }
                InstallProgress::Extracting => bar.set_message(tr!("progress.extracting")),
                InstallProgress::Done => bar.finish_and_clear(),
            }
            let throttled = matches!(p, InstallProgress::Downloading { .. }) && last_emit.elapsed().as_millis() < 250;
            if json && !throttled {
                last_emit = std::time::Instant::now();
                println!("{}", json!({ "event": "install", "progress": p }));
            }
        },
        &CancelToken::new(),
    )?;
    bar.finish_and_clear();

    if ui.json {
        ui.emit(&json!({ "event": "installed", "report": report }));
        return Ok(ExitCode::SUCCESS);
    }
    let verification = match report.manifest.verification {
        ivsr_update::Verification::Sha256 => tr!("verify.sha256"),
        ivsr_update::Verification::SizeOnly => tr!("verify.size_only"),
    };
    eprintln!("{} {} {}", ui::ok_mark(), tr!("engines.installed_ok", name = view.info.name), style(&report.manifest.release).dim());
    eprintln!("  {}", report.executable.display());
    eprintln!("  {}", style(verification).dim());
    Ok(ExitCode::SUCCESS)
}
