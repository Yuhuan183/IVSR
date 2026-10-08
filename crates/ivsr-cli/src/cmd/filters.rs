use std::collections::HashSet;
use std::process::ExitCode;
use std::time::Instant;

use console::style;
use ivsr_core::{FilterChain, FilterStage, FilterStep, MediaKind, ParamValue, ParamValues};
use ivsr_service::{ConflictPolicy, FilterJobRequest, Service};
use serde_json::json;

use super::CmdResult;
use super::engines::param_kind;
use crate::cli::{FilterApplyArgs, FiltersAction};
use crate::i18n::text;
use crate::tr;
use crate::ui::{self, Ui};

const NAME_WIDTH: usize = 28;

type Failure = Box<dyn std::error::Error>;

/// A parsed `-F ID.KEY=VALUE`.
pub(crate) struct FilterParam {
    pub id: String,
    key: String,
    value: ParamValue,
}

pub fn run(service: &Service, action: Option<FiltersAction>, ui: &Ui) -> CmdResult {
    match action.unwrap_or(FiltersAction::List) {
        FiltersAction::List => list(service, ui),
        FiltersAction::Show { filter } => show(service, &filter, ui),
        FiltersAction::Apply(args) => apply(service, &args, ui),
    }
}

pub(crate) fn stage_name(stage: FilterStage) -> &'static str {
    match stage {
        FilterStage::Pre => tr!("filters.stage_pre"),
        FilterStage::Post => tr!("filters.stage_post"),
    }
}

/// `a → b → c` for the enabled steps.
pub(crate) fn steps_text<'a>(ids: impl IntoIterator<Item = &'a str>) -> String {
    ids.into_iter().collect::<Vec<_>>().join(" → ")
}

/// Typed `-F` values, checked against each filter's schema.
pub(crate) fn parse_params(service: &Service, pairs: &[String]) -> Result<Vec<FilterParam>, Failure> {
    pairs
        .iter()
        .map(|pair| {
            let bad = || tr!("filters.bad_param", pair = pair);
            let (target, raw) = pair.split_once('=').ok_or_else(bad)?;
            let (id, key) = target.trim().split_once('.').ok_or_else(bad)?;
            let filter =
                ivsr_core::filter::find(service.registry().filters(), id).ok_or_else(|| tr!("filters.unknown", id = id))?;
            let parsed = ParamValues::parse_pairs(&filter.params(), &[format!("{key}={raw}")])?;
            let value = parsed.into_values().next().ok_or_else(bad)?;
            Ok(FilterParam { id: id.into(), key: key.into(), value })
        })
        .collect()
}

/// Steps named by a comma-separated list, keeping the parameters `configured`
/// gives the same filter.
pub(crate) fn steps_from_list(list: &str, configured: &[FilterStep]) -> Vec<FilterStep> {
    list.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| match configured.iter().find(|s| s.id == id) {
            Some(step) => FilterStep { enabled: true, ..step.clone() },
            None => FilterStep::new(id),
        })
        .collect()
}

/// Sets `params` on every enabled step of the same filter; returns the ids
/// applied, so parameters for filters that will not run can be reported.
pub(crate) fn set_params(steps: &mut [FilterStep], params: &[FilterParam]) -> HashSet<String> {
    let mut applied = HashSet::new();
    for p in params {
        for step in steps.iter_mut().filter(|s| s.enabled && s.id == p.id) {
            step.params.insert(p.key.clone(), p.value.clone());
            applied.insert(p.id.clone());
        }
    }
    applied
}

/// The chain one `upscale` run uses for `stage`, or `None` to keep the configured one.
pub(crate) fn chain_for_run(
    service: &Service,
    stage: FilterStage,
    flag: Option<&str>,
    off: bool,
    params: &[FilterParam],
    applied: &mut HashSet<String>,
) -> Option<FilterChain> {
    if off {
        return Some(FilterChain { enabled: false, steps: None });
    }
    let enabled = service.config().filters.chain(stage).enabled || flag.is_some();
    let mut steps = service.filter_steps(stage);
    if let Some(list) = flag.filter(|l| !l.is_empty()) {
        steps = steps_from_list(list, &steps);
    }
    let set = if enabled { set_params(&mut steps, params) } else { HashSet::new() };
    let changed = flag.is_some() || !set.is_empty();
    applied.extend(set);
    changed.then_some(FilterChain { enabled, steps: Some(steps) })
}

fn list(service: &Service, ui: &Ui) -> CmdResult {
    let views = service.filter_views();
    let config = &service.config().filters;
    if ui.json {
        let chain = |stage| {
            let chain = config.chain(stage);
            json!({ "enabled": chain.enabled, "custom": chain.steps.is_some(), "steps": service.filter_steps(stage) })
        };
        ui.emit(&json!({ "filters": views, "pre": chain(FilterStage::Pre), "post": chain(FilterStage::Post) }));
        return Ok(ExitCode::SUCCESS);
    }
    let rows: Vec<Vec<String>> = views
        .iter()
        .map(|v| {
            let stages: Vec<&str> = v.info.stages.iter().map(|s| stage_name(*s)).collect();
            vec![v.info.id.clone(), text(&v.info.name).to_string(), stages.join(", "), text(&v.info.description).to_string()]
        })
        .collect();
    ui::table(
        &[tr!("filters.col_id"), tr!("filters.col_name"), tr!("filters.col_stages"), tr!("filters.col_description")],
        &rows,
    );
    println!();
    for stage in [FilterStage::Pre, FilterStage::Post] {
        let chain = config.chain(stage);
        let state = if chain.enabled { style(tr!("filters.on")).green() } else { style(tr!("filters.off")).dim() };
        let steps = service.filter_steps(stage);
        let enabled = steps_text(steps.iter().filter(|s| s.enabled).map(|s| s.id.as_str()));
        let origin = if chain.steps.is_none() { format!("  {}", style(tr!("filters.builtin_order")).dim()) } else { String::new() };
        println!("{}: {state} · {enabled}{origin}", stage_name(stage));
    }
    ui.hint(tr!("filters.hint"));
    Ok(ExitCode::SUCCESS)
}

fn show(service: &Service, id: &str, ui: &Ui) -> CmdResult {
    let views = service.filter_views();
    let view = views.iter().find(|v| v.info.id == id).ok_or_else(|| tr!("filters.unknown", id = id))?;
    if ui.json {
        ui.emit(view);
        return Ok(ExitCode::SUCCESS);
    }
    ui.heading(&format!("{} ({})", text(&view.info.name), view.info.id));
    println!("  {}", text(&view.info.description));
    let stages: Vec<&str> = view.info.stages.iter().map(|s| stage_name(*s)).collect();
    println!("  {}", tr!("filters.stages_line", stages = stages.join(", ")));
    println!();
    ui.heading(&tr!("filters.params_heading", id = id));
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

fn apply(service: &Service, args: &FilterApplyArgs, ui: &Ui) -> CmdResult {
    let stage: FilterStage = args.stage.into();
    let params = parse_params(service, &args.filter_params)?;
    let configured = service.filter_steps(stage);
    let mut steps = match &args.steps {
        Some(list) => steps_from_list(list, &configured),
        None => configured,
    };
    let applied = set_params(&mut steps, &params);
    if let Some(p) = params.iter().find(|p| !applied.contains(&p.id)) {
        return Err(tr!("filters.not_in_steps", id = p.id).into());
    }
    let request = FilterJobRequest {
        stage,
        steps: Some(steps),
        reference: args.reference.clone(),
        output: args.output.clone(),
        suffix: args.suffix.clone(),
        image_format: args.format.clone(),
        image_quality: args.quality,
        conflict: if args.overwrite {
            Some(ConflictPolicy::Overwrite)
        } else if args.skip_existing {
            Some(ConflictPolicy::Skip)
        } else {
            None
        },
        recursive: args.recursive,
    };
    let prepared = service.prepare_filters(&args.inputs, &request)?;
    let chain = steps_text(prepared.steps.iter().map(|s| s.id.as_str()));
    let reference_note = |r: &Option<std::path::PathBuf>| match r {
        Some(path) => tr!("filters.reference", path = ui::short_name(path, NAME_WIDTH)),
        None => tr!("filters.no_reference").to_string(),
    };

    if args.dry_run || ui.json {
        if ui.json {
            ui.emit(&json!({ "event": "plan", "stage": prepared.stage, "steps": prepared.steps, "jobs": prepared.jobs }));
        } else {
            ui.heading(&format!("{} · {} · {chain}", tr!("upscale.plan"), stage_name(stage)));
            let rows: Vec<Vec<String>> = prepared
                .jobs
                .iter()
                .map(|j| match skip_reason(j) {
                    Some(reason) => vec![j.planned.input.display().to_string(), style(reason).dim().to_string(), String::new()],
                    None => vec![
                        j.planned.input.display().to_string(),
                        format!("→ {}", j.planned.output.display()),
                        reference_note(&j.reference),
                    ],
                })
                .collect();
            ui::table(&[tr!("upscale.col_input"), tr!("upscale.col_output"), tr!("filters.col_reference")], &rows);
        }
        if args.dry_run {
            return Ok(ExitCode::SUCCESS);
        }
    }

    let runnable: Vec<_> = prepared.runnable().collect();
    if !ui.json && !ui.quiet {
        eprintln!("{} {} · {chain} · {}", style("IVSR").bold(), stage_name(stage), tr!("upscale.files", count = runnable.len()));
    }
    for job in prepared.jobs.iter() {
        if let Some(reason) = skip_reason(job) {
            if ui.json {
                ui.emit(&json!({ "event": "skipped", "input": job.planned.input, "message": reason }));
            } else if !ui.quiet {
                eprintln!("{} {}  {}", ui::skip_mark(), ui::short_name(&job.planned.input, NAME_WIDTH), style(reason).dim());
            }
        }
    }

    let started = Instant::now();
    let mut failed = 0usize;
    for (index, job) in runnable.iter().enumerate() {
        let job_started = Instant::now();
        match service.run_filters(&prepared, job) {
            Ok(outcome) => {
                if ui.json {
                    ui.emit(&json!({ "event": "completed", "index": index, "outcome": outcome, "reference": job.reference }));
                } else {
                    eprintln!(
                        "{} {} {} {}  {}",
                        ui::ok_mark(),
                        ui::short_name(&job.planned.input, NAME_WIDTH),
                        style("→").dim(),
                        outcome.output.display(),
                        style(format!(
                            "{}×{} · {} · {}",
                            outcome.width,
                            outcome.height,
                            reference_note(&job.reference),
                            ui::human_duration(job_started.elapsed().as_millis() as u64)
                        ))
                        .dim()
                    );
                }
            }
            Err(e) => {
                failed += 1;
                if ui.json {
                    ui.emit(&json!({ "event": "failed", "index": index, "error": e.to_string() }));
                } else {
                    eprintln!("{} {}  {}", ui::fail_mark(), ui::short_name(&job.planned.input, NAME_WIDTH), style(e).red());
                }
            }
        }
    }
    let succeeded = runnable.len() - failed;
    let skipped = prepared.jobs.len() - runnable.len();
    let elapsed = started.elapsed().as_millis() as u64;
    if ui.json {
        ui.emit(&json!({ "event": "summary", "succeeded": succeeded, "failed": failed, "skipped": skipped, "elapsed_ms": elapsed }));
    } else if !ui.quiet && (prepared.jobs.len() > 1 || failed > 0) {
        let mut parts = vec![tr!("upscale.done", count = succeeded)];
        if failed > 0 {
            parts.push(style(tr!("upscale.failed", count = failed)).red().to_string());
        }
        if skipped > 0 {
            parts.push(tr!("upscale.skipped", count = skipped));
        }
        eprintln!("{}", tr!("upscale.summary", parts = parts.join(", "), elapsed = ui::human_duration(elapsed)));
    }
    Ok(if failed > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn skip_reason(job: &ivsr_service::FilterJob) -> Option<String> {
    if job.planned.kind == MediaKind::Video {
        return Some(tr!("filters.video_skipped").to_string());
    }
    job.planned.skip.as_ref().map(super::upscale::skip_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_only_land_on_enabled_steps() {
        let mut steps = vec![FilterStep { enabled: false, ..FilterStep::new("saturation") }, FilterStep::new("detail-sharpen")];
        let params = [
            FilterParam { id: "saturation".into(), key: "amount".into(), value: ParamValue::Float(1.3) },
            FilterParam { id: "detail-sharpen".into(), key: "amount".into(), value: ParamValue::Float(1.4) },
        ];
        let applied = set_params(&mut steps, &params);
        assert_eq!(applied, HashSet::from(["detail-sharpen".to_string()]));
        assert!(steps[0].params.is_empty());
    }
}
