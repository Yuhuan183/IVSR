use std::path::PathBuf;
use std::process::ExitCode;

use console::style;
use ivsr_core::MediaKind;
use ivsr_service::Service;
use serde_json::json;

use super::CmdResult;
use crate::i18n::text;
use crate::tr;
use crate::ui::{self, Ui};

fn yes_no(v: bool) -> String {
    if v { style(tr!("ui.yes")).green().to_string() } else { style(tr!("ui.no")).dim().to_string() }
}

pub fn formats(service: &Service, ui: &Ui) -> CmdResult {
    let registry = service.registry();
    let formats = registry.formats();
    let codecs = registry.codecs();
    let video_status = registry.video().status();
    if ui.json {
        ui.emit(&json!({ "formats": formats, "codecs": codecs, "video": video_status }));
        return Ok(ExitCode::SUCCESS);
    }
    for (kind, title) in [(MediaKind::Image, tr!("formats.images")), (MediaKind::Video, tr!("formats.containers"))] {
        ui.heading(title);
        let rows: Vec<Vec<String>> = formats
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| {
                vec![
                    f.id.clone(),
                    f.extensions.join(", "),
                    yes_no(f.decode),
                    yes_no(f.encode),
                    f.note.as_ref().map(|n| text(n).to_string()).unwrap_or_default(),
                ]
            })
            .collect();
        ui::table(
            &[
                tr!("formats.col_id"),
                tr!("formats.col_extensions"),
                tr!("formats.col_read"),
                tr!("formats.col_write"),
                tr!("formats.col_note"),
            ],
            &rows,
        );
        println!();
    }
    ui.heading(tr!("formats.codecs"));
    let rows: Vec<Vec<String>> = codecs
        .iter()
        .map(|c| {
            let quality = c.quality.as_ref().map(|q| format!("{} {}-{} ({})", q.label, q.min, q.max, q.default)).unwrap_or_default();
            vec![c.id.clone(), c.label.clone(), c.containers.join(", "), quality, yes_no(c.available)]
        })
        .collect();
    ui::table(
        &[
            tr!("formats.col_id"),
            tr!("formats.col_codec"),
            tr!("formats.col_containers"),
            tr!("formats.col_quality"),
            tr!("formats.col_available"),
        ],
        &rows,
    );
    if let Some(problem) = video_status.problem() {
        ui.warn(&tr!("formats.video_unavailable", problem = problem));
    }
    Ok(ExitCode::SUCCESS)
}

pub fn probe(service: &Service, files: &[PathBuf], ui: &Ui) -> CmdResult {
    let registry = service.registry();
    let mut failed = false;
    for file in files {
        let info = match registry.classify(file) {
            Some((MediaKind::Image, _)) => registry.images().probe(file).map(|i| json!({ "kind": "image", "info": i })),
            Some((MediaKind::Video, _)) => registry.video().probe(file).map(|i| json!({ "kind": "video", "info": i })),
            None => Err(ivsr_core::Error::UnsupportedFormat(file.display().to_string())),
        };
        match info {
            Ok(value) if ui.json => ui.emit(&json!({ "path": file, "media": value })),
            Ok(value) => print_media(file, &value),
            Err(e) => {
                failed = true;
                if ui.json {
                    ui.emit(&json!({ "path": file, "error": e.to_string() }));
                } else {
                    eprintln!("{} {}  {}", ui::fail_mark(), file.display(), style(e).red());
                }
            }
        }
    }
    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn print_media(file: &std::path::Path, value: &serde_json::Value) {
    let i = &value["info"];
    let size = std::fs::metadata(file).map(|m| ui::human_bytes(m.len())).unwrap_or_default();
    println!("{}  {}", style(file.display()).bold(), style(size).dim());
    if value["kind"] == "image" {
        let alpha = if i["has_alpha"].as_bool() == Some(true) { tr!("ui.yes") } else { tr!("ui.no") };
        println!(
            "  {}",
            tr!("probe.image", w = i["width"], h = i["height"], format = i["format"].as_str().unwrap_or("?"), alpha = alpha)
        );
    } else {
        let fps = i["fps_num"].as_f64().unwrap_or(0.0) / i["fps_den"].as_f64().unwrap_or(1.0).max(1.0);
        let audio = match i["audio"].as_object() {
            Some(a) => format!("{} {}ch", a["codec"].as_str().unwrap_or("?"), a["channels"]),
            None => tr!("probe.no_audio").to_string(),
        };
        let frames = i["frame_count"].as_u64().map(|f| f.to_string()).unwrap_or_else(|| "?".into());
        println!(
            "  {}",
            tr!(
                "probe.video",
                w = i["width"],
                h = i["height"],
                fps = format!("{fps:.3}"),
                codec = i["codec"].as_str().unwrap_or("?"),
                frames = tr!("probe.frames", count = frames),
                duration = format!("{:.2}", i["duration"].as_f64().unwrap_or(0.0)),
                audio = audio
            )
        );
    }
}
