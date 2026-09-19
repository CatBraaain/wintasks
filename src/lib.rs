//! Declarative Task Scheduler XML CLI.

pub mod apply;
pub mod cron;
pub mod def;
pub mod hash;
pub mod render;
pub mod schtasks;
pub mod trigger;
pub mod xml;

use std::io::Write;

use chrono::Local;

use crate::apply::{run_apply, ApplyOptions};
use crate::def::parse_defs;
use crate::render::render_output;
use crate::schtasks::CommandSchtasks;

/// Local wall-clock time for this run. Passed through the conversion pipeline
/// so that tests can pin the date used for StartBoundary.
pub fn now_local() -> chrono::NaiveDateTime {
    Local::now().naive_local()
}

pub const USAGE: &str = "usage: wintasks render [--path <path>]
       wintasks apply [--path <path>] [--mount <folder>] [--dry-run] [--prune]";

/// Spec defaults for `--path` and `--mount`.
const DEFAULT_PATH: &str = "wintasks.yaml";
const DEFAULT_MOUNT: &str = "WinTasks";

/// Dispatches a CLI invocation, writing command output to `out` and `err`.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let Some((command, rest)) = args.split_first() else {
        let _ = writeln!(err, "{USAGE}");
        return 2;
    };
    match command.as_str() {
        "render" => run_render(rest, out, err),
        "apply" => run_apply_command(rest, out, err),
        _ => {
            let _ = writeln!(err, "wintasks: unknown command `{command}`\n{USAGE}");
            2
        }
    }
}

fn run_render(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let RenderCli { path } = match parse_render(args) {
        Ok(cli) => cli,
        Err(message) => return usage_error(&message, err),
    };
    let Some(text) = read_yaml(&path, err) else {
        return 1;
    };
    let defs = match parse_defs(&text, &path) {
        Ok(defs) => defs,
        Err(e) => return error(&e.to_string(), err),
    };
    match render_output(&defs, now_local()) {
        Ok(xml) => {
            let _ = out.write_all(xml.as_bytes());
            0
        }
        Err(e) => error(&format!("{path}: XML generation failed for {e}"), err),
    }
}

fn run_apply_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let ApplyCli { path, opts } = match parse_apply(args) {
        Ok(cli) => cli,
        Err(message) => return usage_error(&message, err),
    };
    let Some(text) = read_yaml(&path, err) else {
        return 1;
    };
    let mut sched = CommandSchtasks;
    run_apply(&text, &path, &opts, &mut sched, now_local(), out, err) as u8
}

/// A parsed `render` invocation.
pub struct RenderCli {
    pub path: String,
}

/// A parsed `apply` invocation.
pub struct ApplyCli {
    pub path: String,
    pub opts: ApplyOptions,
}

pub fn parse_render(args: &[String]) -> Result<RenderCli, String> {
    let mut path = DEFAULT_PATH.to_string();
    parse_args(args, &mut path, &mut None)?;
    Ok(RenderCli { path })
}

pub fn parse_apply(args: &[String]) -> Result<ApplyCli, String> {
    let mut path = DEFAULT_PATH.to_string();
    let mut opts = ApplyOptions {
        mount: DEFAULT_MOUNT.to_string(),
        dry_run: false,
        prune: false,
    };
    parse_args(args, &mut path, &mut Some(&mut opts))?;
    Ok(ApplyCli { path, opts })
}

/// Parses `--path` (both commands) and apply-only options; apply-only
/// options are rejected when `apply_opts` is None (render).
fn parse_args(
    args: &[String],
    path: &mut String,
    apply_opts: &mut Option<&mut ApplyOptions>,
) -> Result<(), String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--path" => *path = value_of(args, &mut i, "--path")?,
            "--mount" => {
                let opts = apply_only(apply_opts, arg)?;
                opts.mount = value_of(args, &mut i, "--mount")?;
            }
            "--dry-run" => apply_only(apply_opts, arg)?.dry_run = true,
            "--prune" => apply_only(apply_opts, arg)?.prune = true,
            _ => return Err(format!("unexpected argument `{arg}`")),
        }
        i += 1;
    }
    Ok(())
}

/// The apply options, or an error when `arg` is apply-only and this is a
/// render invocation.
fn apply_only<'a>(
    apply_opts: &'a mut Option<&mut ApplyOptions>,
    arg: &str,
) -> Result<&'a mut ApplyOptions, String> {
    apply_opts
        .as_deref_mut()
        .ok_or_else(|| format!("{arg} is only valid for apply"))
}

fn value_of(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{name} requires a value"))
}

/// Reads the YAML file, reporting errors as exit code 1.
fn read_yaml(path: &str, err: &mut dyn Write) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) => {
            error(&format!("{path}: {e}"), err);
            None
        }
    }
}

/// Prints `wintasks: <message>` to stderr and returns exit code 1.
fn error(message: &str, err: &mut dyn Write) -> u8 {
    let _ = writeln!(err, "wintasks: {message}");
    1
}

/// Prints the message with usage to stderr and returns exit code 2.
fn usage_error(message: &str, err: &mut dyn Write) -> u8 {
    let _ = writeln!(err, "wintasks: {message}\n{USAGE}");
    2
}
