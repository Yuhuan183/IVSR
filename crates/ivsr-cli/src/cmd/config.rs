use std::process::ExitCode;

use ivsr_service::Service;
use serde_json::json;

use super::CmdResult;
use crate::cli::ConfigAction;
use crate::tr;
use crate::ui::Ui;

pub fn run(service: &Service, action: Option<ConfigAction>, ui: &Ui) -> CmdResult {
    match action.unwrap_or(ConfigAction::Show) {
        ConfigAction::Show => {
            if ui.json {
                ui.emit(service.config());
            } else {
                print!("{}", toml::to_string_pretty(service.config())?);
            }
        }
        ConfigAction::Path => {
            let paths = service.paths();
            if ui.json {
                ui.emit(&json!({ "config": service.config_file(), "paths": paths }));
            } else {
                let pad = |s: &str| console::pad_str(s, 8, console::Alignment::Left, None).into_owned();
                println!("{}{}", pad(tr!("config.config")), service.config_file().display());
                println!("{}{}", pad(tr!("config.data")), paths.data_dir.display());
                println!("{}{}", pad(tr!("config.cache")), paths.cache_dir.display());
            }
        }
        ConfigAction::Get { key } => match service.config().get(&key)? {
            Some(value) if ui.json => ui.emit(&value),
            Some(toml::Value::String(s)) => println!("{s}"),
            Some(value) => println!("{value}"),
            None => {
                ui.info(&tr!("config.not_set", key = key));
                return Ok(ExitCode::FAILURE);
            }
        },
        ConfigAction::Set { key, value } => {
            let mut config = service.config().clone();
            config.set(&key, &value)?;
            config.save(service.config_file())?;
            ui.info(&format!("{key} = {}", config.get(&key)?.map(|v| v.to_string()).unwrap_or_default()));
        }
        ConfigAction::Unset { key } => {
            let mut config = service.config().clone();
            config.unset(&key)?;
            config.save(service.config_file())?;
            ui.info(&tr!("config.reset", key = key));
        }
    }
    Ok(ExitCode::SUCCESS)
}
