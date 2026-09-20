//! Declarative Task Scheduler synchronization CLI.

pub mod apply;
pub mod cron;
pub mod def;
pub mod schtasks;
pub mod trigger;
pub mod xml;

use std::io::Write;

use chrono::Local;

use crate::apply::{SyncOptions, run_sync};
use crate::def::parse_defs;
use crate::schtasks::CommandSchtasks;

const DEFAULT_DEFINITIONS_FILE: &str = "wintasks.yaml";
pub const USAGE: &str =
    "usage: wintasks --mount FOLDER [--dry-run] [--path FILE]\n       wintasks [--help | -h]";
pub const HELP: &str = "wintasks - synchronize Windows Task Scheduler tasks from wintasks.yaml\n\nUsage: wintasks --mount FOLDER [OPTIONS]\n\nOptions:\n  --mount FOLDER  Synchronize tasks under this Task Scheduler folder\n  --dry-run       Show planned changes without modifying the system\n  --path FILE     Read task definitions from FILE (default: wintasks.yaml)\n  -h, --help      Show this help message\n";

pub fn now_local() -> chrono::NaiveDateTime {
    Local::now().naive_local()
}

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let options = match parse_cli(args) {
        Ok(options) => options,
        Err(message) => return usage_error(&message, err),
    };
    if options.help {
        let _ = out.write_all(HELP.as_bytes());
        return 0;
    }
    let definitions_text = match std::fs::read_to_string(&options.path) {
        Ok(text) => text,
        Err(read_error) => return error(&format!("{}: {read_error}", options.path), err),
    };
    let mount = options
        .mount
        .as_deref()
        .expect("mount is required unless help is requested");
    let definitions = match parse_defs(&definitions_text, &options.path, mount) {
        Ok(definitions) => definitions,
        Err(parse_error) => return error(&parse_error.to_string(), err),
    };
    let mut scheduler = CommandSchtasks;
    run_sync(
        &definitions,
        &options.path,
        &SyncOptions {
            dry_run: options.dry_run,
        },
        &mut scheduler,
        now_local(),
        out,
        err,
    ) as u8
}

#[derive(Debug, PartialEq, Eq)]
pub struct CliOptions {
    pub dry_run: bool,
    pub mount: Option<String>,
    pub path: String,
    pub help: bool,
}

pub fn parse_cli(args: &[String]) -> Result<CliOptions, String> {
    let mut options = CliOptions {
        dry_run: false,
        mount: None,
        path: DEFAULT_DEFINITIONS_FILE.to_string(),
        help: false,
    };
    let mut arguments = args.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dry-run" => options.dry_run = true,
            "--mount" => {
                let mount = arguments
                    .next()
                    .ok_or_else(|| "--mount requires a folder path".to_string())?;
                if mount.starts_with('-') {
                    return Err("--mount requires a folder path".to_string());
                }
                options.mount = Some(mount.clone());
            }
            "--path" => {
                options.path = arguments
                    .next()
                    .ok_or_else(|| "--path requires a file path".to_string())?
                    .clone();
            }
            "--help" | "-h" => options.help = true,
            _ => return Err(format!("unexpected argument `{argument}`")),
        }
    }
    if !options.help && options.mount.is_none() {
        return Err("--mount requires a folder path".to_string());
    }
    Ok(options)
}

fn error(message: &str, err: &mut dyn Write) -> u8 {
    let _ = writeln!(err, "wintasks: {message}");
    1
}

fn usage_error(message: &str, err: &mut dyn Write) -> u8 {
    let _ = writeln!(err, "wintasks: {message}\n{USAGE}");
    2
}
