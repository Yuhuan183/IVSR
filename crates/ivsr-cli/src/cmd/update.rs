use std::process::ExitCode;

use console::style;
use ivsr_core::CancelToken;
use ivsr_service::{Flavor, Service, VERSION};
use ivsr_update::UpdateCheck;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;

use super::CmdResult;
use crate::cli::UpdateAction;
use crate::tr;
use crate::ui::{self, Ui};

const EXECUTABLE: &str = if cfg!(windows) { "ivsr.exe" } else { "ivsr" };

/// What the command does once it knows a newer version exists.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `ivsr update`: ask, then install.
    Ask,
    Check,
    Skip,
    Install { yes: bool },
}

pub fn run(service: &Service, action: Option<UpdateAction>, ui: &Ui) -> CmdResult {
    let updates = service.updates(Flavor::Cli, VERSION);
    if !updates.is_configured() {
        if ui.json {
            ui.emit(&json!({ "status": "not_configured" }));
        } else {
            ui.warn(tr!("update.not_configured"));
            ui.hint(tr!("update.configure_hint"));
        }
        return Ok(ExitCode::FAILURE);
    }
    let mode = match action {
        None => Mode::Ask,
        Some(UpdateAction::Check) => Mode::Check,
        Some(UpdateAction::Skip) => Mode::Skip,
        Some(UpdateAction::Install { yes }) => Mode::Install { yes },
    };
    let check = updates.check()?;
    // JSON callers get the check itself; installing reports its own events.
    if ui.json && !matches!(mode, Mode::Install { .. }) {
        ui.emit(&check);
    }
    let UpdateCheck::Available { latest, release, asset, .. } = check else {
        if !ui.json {
            eprintln!("{} {}", ui::ok_mark(), tr!("update.up_to_date", version = VERSION));
        }
        return Ok(ExitCode::SUCCESS);
    };
    if !ui.json {
        eprintln!("{} {}", style("↑").cyan().bold(), tr!("update.available", latest = style(&latest).bold(), current = VERSION));
        if let Some(url) = &release.page_url {
            eprintln!("  {}", style(url).dim());
        }
        for line in release.notes.lines().filter(|l| !l.trim().is_empty()).take(8) {
            eprintln!("  {}", style(line).dim());
        }
    }

    let yes = match mode {
        Mode::Check => {
            if !ui.json {
                ui.hint(tr!("update.install_hint"));
            }
            return Ok(ExitCode::SUCCESS);
        }
        Mode::Skip => {
            updates.skip(&latest)?;
            ui.info(&tr!("update.skipped", version = latest));
            return Ok(ExitCode::SUCCESS);
        }
        // Reporting is all `ivsr update` can do in JSON mode.
        Mode::Ask if ui.json => return Ok(ExitCode::SUCCESS),
        Mode::Ask => false,
        Mode::Install { yes } => yes,
    };
    let Some(asset) = asset else {
        ui.warn(&tr!("update.no_asset", tag = release.tag));
        return Ok(if mode == Mode::Ask { ExitCode::SUCCESS } else { ExitCode::FAILURE });
    };
    if !yes {
        if !ui.interactive {
            // Nobody can answer: say how to install instead of guessing.
            ui.hint(tr!("update.yes_hint"));
            return Ok(if mode == Mode::Ask { ExitCode::SUCCESS } else { ExitCode::FAILURE });
        }
        if !ui::confirm(ui, &tr!("update.confirm", version = latest, size = ui::human_bytes(asset.size))) {
            return Ok(ExitCode::SUCCESS);
        }
    }
    let bar = if ui.interactive {
        let bar = ProgressBar::new(asset.size);
        bar.set_style(
            ProgressStyle::with_template("  downloading {bar:28.cyan/blue} {bytes:>9}/{total_bytes:<9}")
                .expect("valid template")
                .progress_chars("━━─"),
        );
        bar
    } else {
        ProgressBar::hidden()
    };
    let downloaded = updates.download(&asset, &mut |received, _| bar.set_position(received), &CancelToken::new())?;
    bar.finish_and_clear();
    let exe = updates.apply_cli(&downloaded.path, EXECUTABLE)?;
    if ui.json {
        ui.emit(&json!({ "status": "installed", "version": latest.to_string(), "path": exe, "verification": downloaded.verification }));
    } else {
        eprintln!("{} {}", ui::ok_mark(), tr!("update.updated", version = latest, path = exe.display()));
    }
    Ok(ExitCode::SUCCESS)
}
