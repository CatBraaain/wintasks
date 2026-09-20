//! Declarative Task Scheduler synchronization CLI.

pub mod apply;
pub mod cron;
pub mod def;
pub mod render;
pub mod schtasks;
pub mod trigger;
pub mod xml;

use std::io::Write;

use chrono::Local;

use crate::apply::{SyncOptions, run_sync};
use crate::def::parse_defs;
use crate::render::render_output;
use crate::schtasks::CommandSchtasks;

const DEFINITIONS_FILE: &str = "wintasks.yaml";
pub const USAGE: &str = "usage: wintasks [--dry-run | --render]";

pub fn now_local() -> chrono::NaiveDateTime {
    Local::now().naive_local()
}

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let options = match parse_cli(args) {
        Ok(options) => options,
        Err(message) => return usage_error(&message, err),
    };
    let definitions_text = match std::fs::read_to_string(DEFINITIONS_FILE) {
        Ok(text) => text,
        Err(read_error) => return error(&format!("{DEFINITIONS_FILE}: {read_error}"), err),
    };
    let definitions = match parse_defs(&definitions_text, DEFINITIONS_FILE) {
        Ok(definitions) => definitions,
        Err(parse_error) => return error(&parse_error.to_string(), err),
    };
    let now = now_local();

    if options.render {
        return match render_output(&definitions.tasks, now) {
            Ok(xml) => {
                let _ = out.write_all(xml.as_bytes());
                0
            }
            Err(xml_error) => error(
                &format!("{DEFINITIONS_FILE}: XML generation failed for {xml_error}"),
                err,
            ),
        };
    }

    let mut scheduler = CommandSchtasks;
    run_sync(
        &definitions,
        DEFINITIONS_FILE,
        &SyncOptions {
            dry_run: options.dry_run,
        },
        &mut scheduler,
        now,
        out,
        err,
    ) as u8
}

#[derive(Debug, PartialEq, Eq)]
pub struct CliOptions {
    pub dry_run: bool,
    pub render: bool,
}

pub fn parse_cli(args: &[String]) -> Result<CliOptions, String> {
    let mut options = CliOptions {
        dry_run: false,
        render: false,
    };
    for argument in args {
        match argument.as_str() {
            "--dry-run" => options.dry_run = true,
            "--render" => options.render = true,
            _ => return Err(format!("unexpected argument `{argument}`")),
        }
    }
    if options.dry_run && options.render {
        return Err("--dry-run and --render cannot be used together".to_string());
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
