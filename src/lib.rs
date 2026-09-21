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
use crate::schtasks::{CommandSchtasks, Schtasks};

const DEFAULT_DEFINITIONS_FILE: &str = "wintasks.yaml";
const DEFAULT_MOUNT_FOLDER: &str = "wintasks";
pub const USAGE: &str = "usage: wintasks [OPTIONS]";
pub const HELP: &str = "wintasks - synchronize Windows Task Scheduler tasks from wintasks.yaml\n\nUsage: wintasks [OPTIONS]\n\nOptions:\n  --mount FOLDER  Synchronize tasks under this Task Scheduler folder (default: wintasks)\n  --dry-run       Show planned changes without modifying the system\n  --path FILE     Read task definitions from FILE (default: wintasks.yaml)\n  -h, --help      Show this help message\n";

pub fn now_local() -> chrono::NaiveDateTime {
    Local::now().naive_local()
}

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let mut scheduler = CommandSchtasks;
    run_with_scheduler(args, out, err, &mut scheduler)
}

fn run_with_scheduler(
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
    scheduler: &mut dyn Schtasks,
) -> u8 {
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
    let mount = options.mount.as_deref().unwrap_or(DEFAULT_MOUNT_FOLDER);
    let definitions = match parse_defs(&definitions_text, &options.path, mount) {
        Ok(definitions) => definitions,
        Err(parse_error) => return error(&parse_error.to_string(), err),
    };
    run_sync(
        &definitions,
        &options.path,
        &SyncOptions {
            dry_run: options.dry_run,
        },
        scheduler,
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
        mount: Some(DEFAULT_MOUNT_FOLDER.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schtasks::SchtasksError;

    struct RecordingSchtasks {
        created: Vec<String>,
    }

    impl Schtasks for RecordingSchtasks {
        fn query(&mut self) -> Result<String, SchtasksError> {
            Ok(String::new())
        }

        fn create(&mut self, path: &str, _: &str) -> Result<(), SchtasksError> {
            self.created.push(path.to_string());
            Ok(())
        }

        fn delete(&mut self, _: &str) -> Result<(), SchtasksError> {
            Ok(())
        }
    }

    fn run_with_mount(mount: Option<&str>) -> (u8, Vec<String>, String) {
        let directory = tempfile::tempdir().unwrap();
        let definitions_path = directory.path().join("tasks.yaml");
        std::fs::write(
            &definitions_path,
            "- name: task\n  trigger: { type: now, value: x }\n  action: { command: cmd.exe }\n",
        )
        .unwrap();
        let mut args = vec![
            "--path".to_string(),
            definitions_path.to_string_lossy().into_owned(),
        ];
        if let Some(mount) = mount {
            args.extend(["--mount".to_string(), mount.to_string()]);
        }
        let mut scheduler = RecordingSchtasks {
            created: Vec::new(),
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_with_scheduler(&args, &mut out, &mut err, &mut scheduler);
        (code, scheduler.created, String::from_utf8(err).unwrap())
    }

    #[test]
    fn sync_uses_wintasks_when_mount_is_omitted() {
        let (code, created, err) = run_with_mount(None);
        assert_eq!(code, 0, "{err}");
        assert_eq!(created, [r"\wintasks\now\task"]);
    }

    #[test]
    fn sync_uses_explicit_mount_folder() {
        let (code, created, err) = run_with_mount(Some("WinTasks"));
        assert_eq!(code, 0, "{err}");
        assert_eq!(created, [r"\WinTasks\now\task"]);
    }
}
