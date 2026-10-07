//! Command-line grammar.

use std::path::PathBuf;

use std::ffi::OsString;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

const AFTER_HELP: &str = "\
All upscale options (scale, model, format, video codec, ...): ivsr upscale --help

Examples:
  ivsr photo.jpg                       upscale x4 next to the input (photo_x4.jpg)
  ivsr -s 2 -f webp shots/ -o out/     every image in shots/, x2, as WebP into out/
  ivsr clip.mp4 --codec h265 --crf 20  upscale a video, re-encode as HEVC
  ivsr engines install realesrgan      download the Real-ESRGAN runtime
  ivsr engines show realesrgan         models and engine parameters (-p KEY=VALUE)";

#[derive(Parser, Debug)]
#[command(name = "ivsr", version, about = "Image and video super-resolution", after_help = AFTER_HELP)]
#[command(override_usage = "ivsr [OPTIONS] <INPUT>...   (same as `ivsr upscale`)\n       ivsr [OPTIONS] <COMMAND>")]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Parses `args`, treating anything that does not start with a subcommand
    /// as `upscale` arguments: `ivsr photo.jpg` == `ivsr upscale photo.jpg`.
    pub fn parse_with_default(args: Vec<OsString>) -> Self {
        let matches = crate::i18n::localize(Self::command()).get_matches_from(with_default_command(args));
        <Self as clap::FromArgMatches>::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
    }
}

/// Inserts `upscale` before the first argument that is neither a global
/// option nor a subcommand name.
pub fn with_default_command(mut args: Vec<OsString>) -> Vec<OsString> {
    let cmd = Cli::command();
    let is_subcommand = |name: &str| {
        name == "help"
            || cmd.get_subcommands().any(|s| s.get_name() == name || s.get_all_aliases().any(|a| a == name))
    };
    let mut i = 1;
    while let Some(arg) = args.get(i).map(|a| a.to_string_lossy().into_owned()) {
        match arg.as_str() {
            "--config" | "--lang" => i += 2,
            "--json" | "--quiet" | "-v" | "--verbose" | "--no-color" => i += 1,
            a if a.starts_with("--config=") || a.starts_with("--lang=") => i += 1,
            "-h" | "--help" | "-V" | "--version" => return args,
            a if is_subcommand(a) => return args,
            _ => {
                args.insert(i, "upscale".into());
                return args;
            }
        }
    }
    args
}

#[derive(Args, Debug, Clone)]
pub struct Global {
    /// Use this configuration file instead of the default one.
    #[arg(long, global = true, value_name = "FILE", env = "IVSR_CONFIG")]
    pub config: Option<PathBuf>,

    /// Emit machine-readable JSON (one event per line for long-running commands).
    #[arg(long, global = true)]
    pub json: bool,

    /// Only print errors and final results.
    #[arg(long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Show engine and tool messages.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Disable colours (also honours NO_COLOR).
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Interface language: en, zh-TW (default: settings, then the OS).
    #[arg(long, global = true, value_name = "LANG", env = "IVSR_LANG")]
    pub lang: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Upscale images and videos (the default command).
    #[command(visible_alias = "up")]
    Upscale(Box<UpscaleArgs>),

    /// List, inspect and install super-resolution engines.
    Engines {
        #[command(subcommand)]
        action: Option<EnginesAction>,
    },

    /// Install, update, remove, import and benchmark models.
    Models {
        /// Engine whose models to manage (default: the configured engine).
        #[arg(short, long, value_name = "ENGINE")]
        engine: Option<String>,

        #[command(subcommand)]
        action: Option<ModelsAction>,
    },

    /// Show hardware, GPUs and tool status.
    System,

    /// Show supported image formats, video containers and codecs.
    Formats,

    /// Show information about media files.
    Probe {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
    },

    /// Check for and install IVSR updates.
    Update {
        #[command(subcommand)]
        action: Option<UpdateAction>,
    },

    /// Show or change settings.
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Generate shell completions.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand, Debug)]
pub enum EnginesAction {
    /// List engines and their status (default).
    List,
    /// Show an engine's models and parameters.
    Show { engine: String },
    /// Download and install an engine runtime.
    Install {
        engine: String,
        /// Reinstall even if the engine is already available.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ModelsAction {
    /// List installed and downloadable models (default).
    List {
        /// Re-download remote catalogues instead of using the daily cache.
        #[arg(long)]
        refresh: bool,
    },
    /// Show a model's details, hardware needs and baseline timings.
    Show { model: String },
    /// Download models from the catalogue.
    Install {
        #[arg(required = true, value_name = "MODEL")]
        models: Vec<String>,
    },
    /// Update installed models whose catalogue files changed.
    Update {
        #[arg(value_name = "MODEL")]
        models: Vec<String>,
        /// Update every model with an update available.
        #[arg(long, conflicts_with = "models")]
        all: bool,
    },
    /// Delete a downloaded or imported model.
    Remove {
        model: String,
        /// Do not ask for confirmation.
        #[arg(short, long)]
        yes: bool,
    },
    /// Add a model from local ncnn files (.param + .bin).
    Import {
        /// Id for the new model (letters, digits, `-`, `_`, `.`).
        id: String,
        /// The ncnn .param file.
        #[arg(long, value_name = "FILE")]
        param: std::path::PathBuf,
        /// The ncnn .bin file.
        #[arg(long, value_name = "FILE")]
        bin: std::path::PathBuf,
        /// Upscale factor the model produces; verified on import.
        #[arg(short, long, value_name = "N")]
        scale: u32,
        /// Display name.
        #[arg(long)]
        name: Option<String>,
        /// Short description.
        #[arg(long)]
        description: Option<String>,
        /// Licence identifier, e.g. CC-BY-4.0.
        #[arg(long)]
        license: Option<String>,
    },
    /// Make a model the default for upscaling.
    Use { model: String },
    /// Time models on this machine (results feed estimates and advice).
    Bench {
        #[arg(value_name = "MODEL")]
        models: Vec<String>,
        /// Benchmark every installed model.
        #[arg(long, conflicts_with = "models")]
        all: bool,
        /// Scale to measure (default: the model's first native scale).
        #[arg(short, long)]
        scale: Option<u32>,
    },
}

#[derive(Subcommand, Debug)]
pub enum UpdateAction {
    /// Check whether a newer version is available (default).
    Check,
    /// Download and install the newest version.
    Install {
        /// Do not ask for confirmation.
        #[arg(short, long)]
        yes: bool,
    },
    /// Stop reminding about the currently available version.
    Skip,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print the effective configuration (default).
    Show,
    /// Print configuration and data locations.
    Path,
    /// Print one setting, e.g. `video.codec`.
    Get { key: String },
    /// Change one setting, e.g. `ivsr config set output.scale 2`.
    Set { key: String, value: String },
    /// Restore one setting to its default.
    Unset { key: String },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum AudioArg {
    Auto,
    Copy,
    Reencode,
    Drop,
}

#[derive(Args, Debug, Clone)]
pub struct UpscaleArgs {
    /// Images, videos, or directories containing them.
    #[arg(required = true, value_name = "INPUT")]
    pub inputs: Vec<PathBuf>,

    /// Output file (single input) or directory. Default: next to each input.
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Output scale, 1-16. Non-native scales are resampled from the model output.
    #[arg(short, long, value_name = "SCALE")]
    pub scale: Option<f64>,

    /// Model id (see `ivsr engines show <engine>`).
    #[arg(short, long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Engine id.
    #[arg(short, long, value_name = "ENGINE")]
    pub engine: Option<String>,

    /// Engine parameter, repeatable (e.g. -p tile=256 -p tta=true).
    #[arg(short = 'p', long = "param", value_name = "KEY=VALUE")]
    pub params: Vec<String>,

    /// Image output format (png, jpg, webp, ...) or `same`.
    #[arg(short, long, value_name = "FORMAT")]
    pub format: Option<String>,

    /// Quality for lossy image formats, 1-100.
    #[arg(short, long, value_name = "1-100", value_parser = clap::value_parser!(u8).range(1..=100))]
    pub quality: Option<u8>,

    /// Search directories recursively, mirroring their structure in --output.
    #[arg(short, long)]
    pub recursive: bool,

    /// File name suffix; {scale}, {model} and {engine} expand. Default: _x{scale}.
    #[arg(long, value_name = "TEMPLATE")]
    pub suffix: Option<String>,

    /// Replace existing output files (default: pick a free name).
    #[arg(long, conflicts_with = "skip_existing")]
    pub overwrite: bool,

    /// Skip inputs whose output file already exists.
    #[arg(long)]
    pub skip_existing: bool,

    /// Video codec: h264, h265, av1, vp9, prores.
    #[arg(long, value_name = "CODEC", help_heading = "Video")]
    pub codec: Option<String>,

    /// Video quality (CRF); lower is better.
    #[arg(long, value_name = "N", help_heading = "Video")]
    pub crf: Option<u32>,

    /// Encoder preset (e.g. medium, slow; 4-12 for AV1).
    #[arg(long, value_name = "PRESET", help_heading = "Video")]
    pub preset: Option<String>,

    /// Audio handling.
    #[arg(long, value_enum, value_name = "MODE", help_heading = "Video")]
    pub audio: Option<AudioArg>,

    /// Output container (mp4, mkv, mov, webm) or `same`.
    #[arg(long, value_name = "ID", help_heading = "Video")]
    pub container: Option<String>,

    /// Frames upscaled per engine run; higher is faster but uses more temporary disk.
    #[arg(long, value_name = "N", help_heading = "Video")]
    pub batch_frames: Option<u32>,

    /// Show what would be done without processing anything.
    #[arg(short = 'n', long)]
    pub dry_run: bool,
}
