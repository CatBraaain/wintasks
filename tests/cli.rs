use wintasks::{USAGE, parse_cli};

const EXPECTED_HELP: &str = "wintasks - synchronize Windows Task Scheduler tasks from wintasks.yaml\n\nUsage: wintasks [OPTIONS]\n\nOptions:\n  --mount FOLDER  Synchronize tasks under this Task Scheduler folder (default: wintasks)\n  --dry-run       Show planned changes without modifying the system\n  --path FILE     Read task definitions from FILE (default: wintasks.yaml)\n  -h, --help      Show this help message\n";

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn mount_defaults_to_wintasks_and_can_be_overridden() {
    assert_eq!(
        parse_cli(&[]).unwrap(),
        wintasks::CliOptions {
            dry_run: false,
            mount: Some("wintasks".to_string()),
            path: "wintasks.yaml".to_string(),
            help: false,
        }
    );
    assert_eq!(
        parse_cli(&args(&["--mount", "WinTasks"])).unwrap().mount,
        Some("WinTasks".to_string())
    );
}

#[test]
fn cli_options_are_parsed() {
    assert!(
        parse_cli(&args(&["--mount", "WinTasks", "--dry-run"]))
            .unwrap()
            .dry_run
    );
    assert!(parse_cli(&args(&["--help"])).unwrap().help);
    assert!(parse_cli(&args(&["-h"])).unwrap().help);
    assert!(
        parse_cli(&args(&["--help", "--dry-run", "--path", "input.yaml"]))
            .unwrap()
            .help
    );
    assert_eq!(
        parse_cli(&args(&["--mount", "Other", "--path", "input.yaml"]))
            .unwrap()
            .path,
        "input.yaml"
    );
    assert_eq!(
        parse_cli(&args(&["--mount", "Other"]))
            .unwrap()
            .mount
            .as_deref(),
        Some("Other")
    );
    for invalid in [
        ["--render"].as_slice(),
        ["apply"].as_slice(),
        ["render"].as_slice(),
        ["--path=input.yaml"].as_slice(),
        ["--path"].as_slice(),
        ["--mount"].as_slice(),
        ["--mount", "--help"].as_slice(),
        ["--mount=Other"].as_slice(),
        ["--prune"].as_slice(),
    ] {
        assert!(parse_cli(&args(invalid)).is_err(), "{invalid:?}");
    }
}

#[test]
fn usage_errors_write_prefix_usage_and_exit_two() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = wintasks::run(&args(&["--unknown"]), &mut stdout, &mut stderr);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!("wintasks: unexpected argument `--unknown`\n{USAGE}\n")
    );
}

#[test]
fn help_does_not_hide_unknown_argument_errors() {
    for values in [&["--help", "--unknown"][..], &["-h", "--unknown"][..]] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = wintasks::run(&args(values), &mut stdout, &mut stderr);
        assert_eq!(code, 2, "args: {values:?}");
        assert!(stdout.is_empty(), "args: {values:?}");
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            format!("wintasks: unexpected argument `--unknown`\n{USAGE}\n"),
            "args: {values:?}"
        );
    }
}

#[test]
fn help_writes_canonical_output_without_reading_definitions() {
    let directory = tempfile::tempdir().unwrap();
    for values in [
        &["--help"][..],
        &["-h"][..],
        &["--help", "--dry-run"][..],
        &["--help", "--path", "custom.yaml"][..],
        &["--help", "--mount", "WinTasks"][..],
        &["--mount", "WinTasks", "--help"][..],
        &["-h", "--dry-run", "--path", "custom.yaml"][..],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
            .current_dir(directory.path())
            .args(values)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "args: {values:?}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), EXPECTED_HELP);
        assert!(output.stderr.is_empty(), "args: {values:?}");
    }
}

#[test]
fn invalid_mount_exits_one_before_scheduler_query() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("custom.yaml"),
        "- name: task\n  trigger: { type: now, value: x }\n  action: { command: cmd.exe }\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
        .current_dir(directory.path())
        .args(["--mount", "\\", "--path", "custom.yaml"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "wintasks: custom.yaml: mount must be a non-root folder path\n"
    );
}

#[test]
fn path_error_uses_the_custom_definition_file() {
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
        .current_dir(directory.path())
        .args(["--mount", "WinTasks", "--path", "custom.yaml"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("wintasks: custom.yaml: "), "{stderr}");
    assert!(!stderr.contains("usage"), "{stderr}");
}

#[test]
fn path_parse_error_uses_the_custom_definition_file() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("custom.yaml"), "mount: WinTasks\n").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
        .current_dir(directory.path())
        .args(["--mount", "WinTasks", "--path", "custom.yaml"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("wintasks: custom.yaml:"), "{stderr}");
    assert!(!stderr.contains("usage"), "{stderr}");
}

#[test]
fn missing_definitions_file_exits_one_with_error_and_no_usage() {
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("wintasks: wintasks.yaml: "), "{stderr}");
    assert!(!stderr.contains("usage"), "{stderr}");
}
