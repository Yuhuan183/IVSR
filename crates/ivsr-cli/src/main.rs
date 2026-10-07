mod cli;
mod cmd;
mod i18n;
mod ui;

use std::process::ExitCode;

use clap::CommandFactory;

use cli::{Cli, Command};
use ui::Ui;

/// Language must be known before parsing so help text can be translated:
/// `--lang`, then `IVSR_LANG`, then the config file, then the OS.
fn early_lang(args: &[std::ffi::OsString]) -> ivsr_service::Lang {
    let value_of = |flag: &str| {
        let prefix = format!("{flag}=");
        args.iter().enumerate().find_map(|(i, a)| {
            let a = a.to_string_lossy();
            if a == flag {
                args.get(i + 1).map(|v| v.to_string_lossy().into_owned())
            } else {
                a.strip_prefix(&prefix).map(str::to_string)
            }
        })
    };
    if let Some(lang) = value_of("--lang").and_then(|l| ivsr_service::Lang::from_locale(&l)) {
        return lang;
    }
    let config_file = value_of("--config")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("IVSR_CONFIG").map(Into::into))
        .unwrap_or_else(|| ivsr_service::AppPaths::discover().config_file);
    let config = ivsr_service::Config::load(&config_file).unwrap_or_default();
    ivsr_service::i18n::resolve(&config)
}

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    i18n::init(early_lang(&args));
    let cli = Cli::parse_with_default(args);
    if cli.global.no_color {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
    let ui = Ui::new(&cli.global);

    if let Some(Command::Completions { shell }) = &cli.command {
        clap_complete::generate(*shell, &mut i18n::localize(Cli::command()), "ivsr", &mut std::io::stdout());
        return ExitCode::SUCCESS;
    }

    let service = match ivsr_service::Service::load(cli.global.config.as_deref()) {
        Ok(s) => s,
        Err(e) => return ui.fail(&e.to_string()),
    };

    let result = match cli.command {
        Some(Command::Upscale(args)) => cmd::upscale::run(service, &args, &ui),
        None => {
            let _ = i18n::localize(Cli::command()).print_help();
            return ExitCode::from(2);
        }
        Some(Command::Engines { action }) => cmd::engines::run(&service, action, &ui),
        Some(Command::Models { engine, action }) => cmd::models::run(&service, engine, action, &ui),
        Some(Command::System) => cmd::system::run(&service, &ui),
        Some(Command::Formats) => cmd::info::formats(&service, &ui),
        Some(Command::Probe { files }) => cmd::info::probe(&service, &files, &ui),
        Some(Command::Update { action }) => cmd::update::run(&service, action, &ui),
        Some(Command::Config { action }) => cmd::config::run(&service, action, &ui),
        Some(Command::Completions { .. }) => unreachable!("handled above"),
    };
    match result {
        Ok(code) => code,
        Err(e) => ui.fail(&e.to_string()),
    }
}
