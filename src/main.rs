//! CLI entry point: argument parsing and command dispatch.

use std::io::Write;
use std::process::ExitCode;

use wintasks::apply::{ApplyOptions, run_apply};
use wintasks::def::parse_defs;
use wintasks::now_local;
use wintasks::render::render_output;
use wintasks::schtasks::CommandSchtasks;

const USAGE: &str = "usage: wintasks render [--path <path>]
       wintasks apply [--path <path>] [--mount <folder>] [--dry-run] [--prune]";

/// Spec defaults for `--path` and `--mount`.
const DEFAULT_PATH: &str = "wintasks.yaml";
const DEFAULT_MOUNT: &str = "WinTasks";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let code = run(&args, &mut stdout, &mut stderr);
    ExitCode::from(code)
}

fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
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
struct RenderCli {
    path: String,
}

/// A parsed `apply` invocation.
struct ApplyCli {
    path: String,
    opts: ApplyOptions,
}

fn parse_render(args: &[String]) -> Result<RenderCli, String> {
    let mut path = DEFAULT_PATH.to_string();
    parse_args(args, &mut path, &mut None)?;
    Ok(RenderCli { path })
}

fn parse_apply(args: &[String]) -> Result<ApplyCli, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the CLI with byte-vector sinks so stdout and stderr are
    /// observable as strings.
    fn run_cli(args: &[&str]) -> (u8, String, String) {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&owned, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    fn temp_path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("wintasks-cli-test-{}-{name}", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn no_arguments_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&[]);
        assert_eq!(code, 2);
        assert!(err.contains("usage: wintasks"), "{err}");
    }

    #[test]
    fn unknown_command_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&["bogus"]);
        assert_eq!(code, 2);
        assert!(err.contains("unknown command `bogus`"), "{err}");
    }

    #[test]
    fn unknown_argument_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&["render", "--bogus"]);
        assert_eq!(code, 2);
        assert!(err.contains("unexpected argument `--bogus`"), "{err}");
    }

    #[test]
    fn option_without_value_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&["apply", "--path"]);
        assert_eq!(code, 2);
        assert!(err.contains("--path requires a value"), "{err}");
    }

    #[test]
    fn render_rejects_apply_only_options() {
        for args in [
            vec!["render", "--mount", "M"],
            vec!["render", "--dry-run"],
            vec!["render", "--prune"],
        ] {
            let (code, _, err) = run_cli(&args);
            assert_eq!(code, 2, "{args:?}");
            assert!(err.contains("only valid for apply"), "{err}");
        }
    }

    #[test]
    fn options_default_to_spec_values() {
        let RenderCli { path } = parse_render(&[]).unwrap();
        assert_eq!(path, "wintasks.yaml");
        let ApplyCli { path, opts } = parse_apply(&[]).unwrap();
        assert_eq!(path, "wintasks.yaml");
        assert_eq!(opts.mount, "WinTasks");
        assert!(!opts.dry_run);
        assert!(!opts.prune);
    }

    #[test]
    fn apply_accepts_apply_only_options() {
        let args = ["--mount", "M", "--dry-run", "--prune"].map(String::from);
        let ApplyCli { opts, .. } = parse_apply(&args).unwrap();
        assert_eq!(opts.mount, "M");
        assert!(opts.dry_run);
        assert!(opts.prune);
    }

    #[test]
    fn render_reads_yaml_from_explicit_path() {
        let path = temp_path("render-explicit.yaml");
        std::fs::write(
            &path,
            "- name: Hello\n  trigger: { type: now, value: v }\n  action: { command: cmd.exe }\n",
        )
        .unwrap();
        let (code, out, _) = run_cli(&["render", "--path", &path]);
        std::fs::remove_file(&path).ok();
        assert_eq!(code, 0);
        assert!(out.contains("--- Hello ---"), "{out}");
    }

    #[test]
    fn rejects_directory_as_path() {
        // Directories are not valid --path values (spec); read_to_string
        // fails with EISDIR.
        let dir = temp_path("not-a-file-dir");
        std::fs::create_dir(&dir).unwrap();
        let (code, _, err) = run_cli(&["render", "--path", &dir]);
        std::fs::remove_dir(&dir).ok();
        assert_eq!(code, 1);
        assert!(err.contains(&dir), "{err}");
    }
}
