pub mod config;
pub mod engines;
pub mod info;
pub mod models;
pub mod system;
pub mod update;
pub mod upscale;

pub type CmdResult = Result<std::process::ExitCode, Box<dyn std::error::Error>>;
