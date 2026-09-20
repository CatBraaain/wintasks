use wintasks::{USAGE, parse_cli};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn no_subcommand_is_the_default_sync_invocation() {
    assert_eq!(
        parse_cli(&[]).unwrap(),
        wintasks::CliOptions {
            dry_run: false,
            render: false,
        }
    );
}

#[test]
fn dry_run_and_render_are_the_only_options() {
    assert!(parse_cli(&args(&["--dry-run"])).unwrap().dry_run);
    assert!(parse_cli(&args(&["--render"])).unwrap().render);
    for invalid in [
        ["--dry-run", "--render"].as_slice(),
        ["apply"].as_slice(),
        ["render"].as_slice(),
        ["--path", "input.yaml"].as_slice(),
        ["--mount", "Other"].as_slice(),
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
fn render_cli_uses_fixed_file_and_writes_ordered_documents() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("wintasks.yaml"),
        "mount: WinTasks\ntasks:\n  - name: first\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n  - name: second\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
        .current_dir(directory.path())
        .arg("--render")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("--- first ---\n<?xml"));
    assert!(stdout.contains("</Task>\n--- second ---\n<?xml"));
    assert!(!stdout.contains("\n\n"));
}

#[test]
fn render_cli_outputs_nothing_for_empty_desired_state() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("wintasks.yaml"),
        "mount: WinTasks\ntasks: []\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
        .current_dir(directory.path())
        .arg("--render")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
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
