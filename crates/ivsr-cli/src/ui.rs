//! Terminal presentation: colours, symbols, tables and JSON output.

use std::io::IsTerminal;
use std::process::ExitCode;

use console::style;
use serde::Serialize;

use crate::cli::Global;

pub struct Ui {
    pub json: bool,
    pub quiet: bool,
    pub verbose: bool,
    /// Whether stderr is an interactive terminal (progress bars, prompts).
    pub interactive: bool,
}

impl Ui {
    pub fn new(global: &Global) -> Self {
        Self {
            json: global.json,
            quiet: global.quiet,
            verbose: global.verbose,
            interactive: std::io::stderr().is_terminal() && !global.json,
        }
    }

    /// Prints one JSON document or event line to stdout.
    pub fn emit<T: Serialize>(&self, value: &T) {
        println!("{}", serde_json::to_string(value).expect("serializable"));
    }

    pub fn heading(&self, text: &str) {
        if !self.json {
            println!("{}", style(text).bold());
        }
    }

    pub fn info(&self, text: &str) {
        if !self.json && !self.quiet {
            eprintln!("{text}");
        }
    }

    pub fn hint(&self, text: &str) {
        if !self.json && !self.quiet {
            eprintln!("{} {}", style(crate::tr!("ui.hint")).cyan().bold(), text);
        }
    }

    pub fn warn(&self, text: &str) {
        if !self.json {
            eprintln!("{} {}", style(crate::tr!("ui.warning")).yellow().bold(), text);
        }
    }

    pub fn fail(&self, text: &str) -> ExitCode {
        if self.json {
            self.emit(&serde_json::json!({ "event": "error", "message": text }));
        } else {
            eprintln!("{} {}", style(crate::tr!("ui.error")).red().bold(), text);
        }
        ExitCode::FAILURE
    }
}

pub fn ok_mark() -> console::StyledObject<&'static str> {
    style("✓").green().bold()
}

pub fn fail_mark() -> console::StyledObject<&'static str> {
    style("✗").red().bold()
}

pub fn skip_mark() -> console::StyledObject<&'static str> {
    style("–").dim()
}

/// Left-aligned text table with a dimmed header row.
pub fn table(header: &[&str], rows: &[Vec<String>]) {
    let columns = header.len();
    let mut widths: Vec<usize> = header.iter().map(|h| console::measure_text_width(h)).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(columns) {
            widths[i] = widths[i].max(console::measure_text_width(cell));
        }
    }
    let line = |cells: Vec<String>| {
        let mut out = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == cells.len() {
                out.push_str(cell);
            } else {
                out.push_str(&console::pad_str(cell, widths[i], console::Alignment::Left, None));
                out.push_str("  ");
            }
        }
        println!("  {}", out.trim_end());
    };
    line(header.iter().map(|h| style(h).dim().to_string()).collect());
    for row in rows {
        line(row.clone());
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn human_duration(ms: u64) -> String {
    let secs = ms as f64 / 1000.0;
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}m{:02}s", ms / 60_000, (ms / 1000) % 60)
    }
}

/// Shortens a file name to `max` display columns, keeping the extension.
pub fn short_name(path: &std::path::Path, max: usize) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    if console::measure_text_width(&name) <= max {
        return name;
    }
    let keep = max.saturating_sub(1);
    let tail: String = name.chars().rev().take(keep / 2).collect::<Vec<_>>().into_iter().rev().collect();
    let head: String = name.chars().take(keep - tail.chars().count()).collect();
    format!("{head}…{tail}")
}

/// Asks a yes/no question on the terminal; non-interactive sessions answer no.
pub fn confirm(ui: &Ui, question: &str) -> bool {
    if !ui.interactive {
        return false;
    }
    eprint!("{question} {} ", crate::tr!("ui.confirm_suffix"));
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok() && matches!(answer.trim(), "y" | "Y" | "yes")
}
