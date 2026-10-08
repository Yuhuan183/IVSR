use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use console::style;
use ivsr_core::scale::format_scale;
use ivsr_core::{AudioMode, CancelToken, FilterStage, LogLevel, ParamValues, Progress, Reporter, Stage, UpscaleSettings};
use ivsr_service::throttle::Throttled;
use ivsr_service::{ConflictPolicy, Flavor, JobRequest, Prepared, Service, VERSION};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde_json::json;

use super::CmdResult;
use super::filters::{chain_for_run, parse_params, stage_name, steps_text};
use crate::cli::{AudioArg, UpscaleArgs};
use crate::tr;
use crate::ui::{self, Ui};

const NAME_WIDTH: usize = 28;

pub fn run(service: Service, args: &UpscaleArgs, ui: &Ui) -> CmdResult {
    let service = Arc::new(service);
    if ui.interactive {
        // Refresh the cached update state in the background; never delays the job.
        let svc = service.clone();
        std::thread::spawn(move || svc.updates(Flavor::Cli, VERSION).check_if_due());
    }

    let request = build_request(&service, args)?;
    let prepared = service.prepare(&args.inputs, &request)?;
    if args.dry_run {
        show_plan(&prepared, ui);
        return Ok(ExitCode::SUCCESS);
    }
    if ui.json {
        ui.emit(&json!({ "event": "plan", "engine": prepared.engine, "settings": prepared.settings, "jobs": prepared.jobs }));
    } else {
        announce(&service, &prepared, ui);
    }

    let cancel = CancelToken::new();
    install_interrupt_handler(cancel.clone());

    let started = Instant::now();
    let runnable: Vec<_> = prepared.runnable().collect();
    let total = runnable.len();
    let (mut succeeded, mut failed) = (0usize, 0usize);
    let skipped = prepared.jobs.len() - total;
    for job in prepared.jobs.iter().filter(|j| j.skip.is_some()) {
        if ui.json {
            ui.emit(&json!({ "event": "skipped", "input": job.input, "reason": job.skip, "message": job.skip.as_ref().map(skip_text) }));
        } else if !ui.quiet {
            eprintln!(
                "{} {}  {}",
                ui::skip_mark(),
                ui::short_name(&job.input, NAME_WIDTH),
                style(job.skip.as_ref().map(skip_text).unwrap_or_default()).dim()
            );
        }
    }

    for (index, (planned, spec)) in runnable.iter().enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        let label = format!("[{}/{}] {}", index + 1, total, ui::short_name(&planned.input, NAME_WIDTH));
        let job_started = Instant::now();
        let result = if ui.json {
            ui.emit(&json!({ "event": "started", "index": index, "input": planned.input, "output": planned.output }));
            let reporter = Throttled::new(JsonReporter { index }, Duration::from_millis(250));
            service.run(spec, &prepared.engine, &reporter, &cancel)
        } else {
            let bar = progress_bar(ui, &label);
            let reporter = Throttled::new(BarReporter { bar: bar.clone(), verbose: ui.verbose }, Duration::from_millis(50));
            let result = service.run(spec, &prepared.engine, &reporter, &cancel);
            bar.finish_and_clear();
            result
        };
        let elapsed = job_started.elapsed().as_millis() as u64;
        match result {
            Ok(outcome) => {
                succeeded += 1;
                if ui.json {
                    ui.emit(&json!({ "event": "completed", "index": index, "outcome": outcome, "elapsed_ms": elapsed }));
                } else {
                    let frames = outcome.frames.map(|f| format!(" · {}", tr!("upscale.frames", count = f))).unwrap_or_default();
                    eprintln!(
                        "{} {} {} {}  {}",
                        ui::ok_mark(),
                        ui::short_name(&planned.input, NAME_WIDTH),
                        style("→").dim(),
                        outcome.output.display(),
                        style(format!("{}×{}{frames} · {}", outcome.width, outcome.height, ui::human_duration(elapsed))).dim()
                    );
                }
            }
            Err(e) if e.is_cancelled() => {
                if ui.json {
                    ui.emit(&json!({ "event": "cancelled", "index": index }));
                }
                break;
            }
            Err(e) => {
                failed += 1;
                if ui.json {
                    ui.emit(&json!({ "event": "failed", "index": index, "error": e.to_string() }));
                } else {
                    eprintln!("{} {}  {}", ui::fail_mark(), ui::short_name(&planned.input, NAME_WIDTH), style(e).red());
                }
            }
        }
    }

    let cancelled = cancel.is_cancelled();
    let elapsed = started.elapsed().as_millis() as u64;
    if ui.json {
        ui.emit(&json!({
            "event": "summary", "succeeded": succeeded, "failed": failed, "skipped": skipped,
            "cancelled": cancelled, "elapsed_ms": elapsed,
        }));
    } else if !ui.quiet && (total + skipped > 1 || cancelled || failed > 0) {
        let mut parts = vec![tr!("upscale.done", count = succeeded)];
        if failed > 0 {
            parts.push(style(tr!("upscale.failed", count = failed)).red().to_string());
        }
        if skipped > 0 {
            parts.push(tr!("upscale.skipped", count = skipped));
        }
        if cancelled {
            parts.push(style(tr!("upscale.cancelled")).yellow().to_string());
        }
        eprintln!("{}", tr!("upscale.summary", parts = parts.join(", "), elapsed = ui::human_duration(elapsed)));
    }
    if !ui.json
        && let Some(version) = service.updates(Flavor::Cli, VERSION).known_update() {
            ui.hint(&tr!("upscale.update_hint", version = version, current = VERSION));
        }

    Ok(if cancelled {
        ExitCode::from(130)
    } else if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn build_request(service: &Service, args: &UpscaleArgs) -> Result<JobRequest, Box<dyn std::error::Error>> {
    let engine_id = args.engine.clone().unwrap_or_else(|| service.config().engine.clone());
    let engine = service.registry().engine(&engine_id)?;
    let params = ParamValues::parse_pairs(&engine.params(), &args.params)?;
    let filter_params = parse_params(service, &args.filter_params)?;
    let mut applied = std::collections::HashSet::new();
    let pre = chain_for_run(service, FilterStage::Pre, args.pre.as_deref(), args.no_pre, &filter_params, &mut applied);
    let post = chain_for_run(service, FilterStage::Post, args.post.as_deref(), args.no_post, &filter_params, &mut applied);
    if let Some(p) = filter_params.iter().find(|p| !applied.contains(&p.id)) {
        return Err(tr!("filters.not_in_chain", id = p.id).into());
    }
    Ok(JobRequest {
        engine: Some(engine_id),
        model: args.model.clone(),
        scale: args.scale,
        params,
        image_format: args.format.clone(),
        image_quality: args.quality,
        video_codec: args.codec.clone(),
        video_quality: args.crf,
        video_preset: args.preset.clone(),
        audio: args.audio.map(|a| match a {
            AudioArg::Auto => AudioMode::Auto,
            AudioArg::Copy => AudioMode::Copy,
            AudioArg::Reencode => AudioMode::Reencode,
            AudioArg::Drop => AudioMode::Drop,
        }),
        container: args.container.clone(),
        batch_frames: args.batch_frames,
        output: args.output.clone(),
        suffix: args.suffix.clone(),
        conflict: if args.overwrite {
            Some(ConflictPolicy::Overwrite)
        } else if args.skip_existing {
            Some(ConflictPolicy::Skip)
        } else {
            None
        },
        recursive: args.recursive,
        pre,
        post,
    })
}

/// ` · post: a → b` for each stage that runs filters.
fn filters_text(settings: &UpscaleSettings) -> String {
    [(FilterStage::Pre, &settings.pre), (FilterStage::Post, &settings.post)]
        .into_iter()
        .filter(|(_, specs)| !specs.is_empty())
        .map(|(stage, specs)| format!(" · {}: {}", stage_name(stage), steps_text(specs.iter().map(|s| s.id.as_str()))))
        .collect()
}

fn announce(service: &Service, prepared: &Prepared, ui: &Ui) {
    if ui.quiet {
        return;
    }
    let engine = service.registry().engine(&prepared.engine).map(|e| e.info().name).unwrap_or_default();
    let count = prepared.jobs.iter().filter(|j| j.skip.is_none()).count();
    eprintln!(
        "{} {} · {} · x{} · {}{}",
        style("IVSR").bold(),
        engine,
        prepared.settings.model,
        format_scale(prepared.settings.scale),
        tr!("upscale.files", count = count),
        filters_text(&prepared.settings)
    );
}

fn show_plan(prepared: &Prepared, ui: &Ui) {
    if ui.json {
        ui.emit(&json!({ "event": "plan", "engine": prepared.engine, "settings": prepared.settings, "jobs": prepared.jobs }));
        return;
    }
    let rows: Vec<Vec<String>> = prepared
        .jobs
        .iter()
        .map(|j| match &j.skip {
            Some(reason) => vec![
                j.input.display().to_string(),
                style(tr!("upscale.skip_prefix", reason = skip_text(reason))).dim().to_string(),
            ],
            None => vec![j.input.display().to_string(), format!("→ {}", j.output.display())],
        })
        .collect();
    ui.heading(&format!(
        "{} · {} · {} · x{}{}",
        tr!("upscale.plan"),
        prepared.engine,
        prepared.settings.model,
        format_scale(prepared.settings.scale),
        filters_text(&prepared.settings)
    ));
    ui::table(&[tr!("upscale.col_input"), tr!("upscale.col_output")], &rows);
}

fn progress_bar(ui: &Ui, label: &str) -> ProgressBar {
    if !ui.interactive || ui.quiet {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::with_draw_target(Some(1000), ProgressDrawTarget::stderr_with_hz(15));
    bar.set_style(
        ProgressStyle::with_template("{prefix} {bar:28.cyan/blue} {percent:>3}% {msg:<22} {elapsed:>4}")
            .expect("valid template")
            .progress_chars("━━─"),
    );
    bar.set_prefix(console::pad_str(label, NAME_WIDTH + 8, console::Alignment::Left, None).into_owned());
    bar.enable_steady_tick(Duration::from_millis(200));
    bar
}

struct BarReporter {
    bar: ProgressBar,
    verbose: bool,
}

impl Reporter for BarReporter {
    fn progress(&self, p: Progress) {
        self.bar.set_position((p.overall * 1000.0) as u64);
        let units = match (p.stage, p.units) {
            (Stage::Upscaling | Stage::Filtering | Stage::Encoding, Some((done, total))) => format!(" {done}/{total}"),
            _ => String::new(),
        };
        self.bar.set_message(format!("{}{units}", stage_text(p.stage)));
    }

    fn log(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Warn => self.bar.println(format!("  {} {message}", style(tr!("ui.warning")).yellow())),
            LogLevel::Info if self.verbose => self.bar.println(format!("  {message}")),
            LogLevel::Debug if self.verbose => self.bar.println(format!("  {}", style(message).dim())),
            _ => {}
        }
    }
}

struct JsonReporter {
    index: usize,
}

impl Reporter for JsonReporter {
    fn progress(&self, p: Progress) {
        println!(
            "{}",
            json!({ "event": "progress", "index": self.index, "stage": p.stage, "overall": p.overall, "units": p.units })
        );
    }

    fn log(&self, level: LogLevel, message: &str) {
        if level != LogLevel::Debug {
            println!("{}", json!({ "event": "log", "index": self.index, "level": level, "message": message }));
        }
    }
}

/// First Ctrl-C cancels gracefully (temporary files are removed); a second exits at once.
fn install_interrupt_handler(cancel: CancelToken) {
    let pressed = AtomicBool::new(false);
    let _ = ctrlc::set_handler(move || {
        if pressed.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
        eprintln!("\n{} {}", style(tr!("upscale.cancelling")).yellow().bold(), tr!("upscale.cancel_hint"));
        cancel.cancel();
    });
}

pub(crate) fn skip_text(reason: &ivsr_service::SkipReason) -> String {
    match reason {
        ivsr_service::SkipReason::Unsupported => tr!("skip.unsupported").to_string(),
        ivsr_service::SkipReason::OutputExists => tr!("skip.output_exists").to_string(),
        ivsr_service::SkipReason::VideoUnavailable { detail } => tr!("skip.video_unavailable", detail = detail),
    }
}

fn stage_text(stage: Stage) -> &'static str {
    match stage {
        Stage::Preparing => tr!("stage.preparing"),
        Stage::Decoding => tr!("stage.decoding"),
        Stage::Upscaling => tr!("stage.upscaling"),
        Stage::Filtering => tr!("stage.filtering"),
        Stage::Encoding => tr!("stage.encoding"),
        Stage::Finalizing => tr!("stage.finalizing"),
    }
}
