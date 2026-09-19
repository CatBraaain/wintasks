//! Integration tests for the `wintasks render` CLI surface.
//!
//! Oracle: SPEC.md — the command overview (`render` is a pure
//! YAML-to-XML converter that changes nothing), the common-options
//! table (render rows), the "invalid arguments print usage and exit
//! nonzero" sentence, and the render rows of the exit-code and
//! error-display sections. One `#[test]` per spec table row / bullet.
//! apply-side behavior is covered by tests/apply.rs; apply-only
//! options appear here only as arguments `render` must reject.

mod common;

use common::{run_render, write_temp_yaml};

/// `- name: Hello` (now trigger) — the minimal desired state.
const ONE_TASK: &str = "\
- name: Hello
  trigger: { type: now, value: v }
  action: { command: cmd.exe }
";

/// Runs the CLI with byte sinks and returns (exit code, stdout, stderr).
fn run_cli(args: &[&str]) -> (u8, String, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = wintasks::run(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

// ------------------------------------------------------ command overview
// Spec: `render` converts YAML to XML on stdout and changes nothing.

#[test]
fn render_writes_xml_to_stdout_and_leaves_input_unchanged() {
    let path = write_temp_yaml("cli-purity", ONE_TASK);
    let before = std::fs::read_to_string(&path).unwrap();
    let (code, out, err) = run_render(&path);
    assert_eq!(code, 0);
    assert!(out.starts_with("--- Hello ---\n"), "{out}");
    assert!(out.contains("<?xml"), "{out}");
    assert!(err.is_empty(), "{err}");
    // A pure converter must not rewrite its input.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

// -------------------------------------------------------- common options
// Spec common-options table, render rows of --path.

#[test]
fn path_defaults_to_wintasks_yaml() {
    // Runs the real binary in a temp cwd so the default is observable.
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("wintasks.yaml"), ONE_TASK).expect("write yaml");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wintasks"))
        .current_dir(dir.path())
        .args(["render"])
        .output()
        .expect("run wintasks binary");
    assert!(output.status.success());
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("--- Hello ---"), "{out}");
}

#[test]
fn duplicate_path_option_last_value_wins() {
    // Spec: duplicate --path uses the last value; the first (missing)
    // path must not fail the run.
    let missing = "/nonexistent/wintasks-first.yaml";
    let path = write_temp_yaml("cli-dup-path", ONE_TASK);
    let (code, out, _) = run_cli(&["render", "--path", missing, "--path", &path]);
    assert_eq!(code, 0);
    assert!(out.contains("--- Hello ---"), "{out}");
}

#[test]
fn directory_path_is_read_error_without_usage() {
    // Spec: --path cannot be a directory; that is a read failure
    // (exit 1), not an invalid-argument usage error.
    let dir = tempfile::tempdir().expect("temp dir");
    let (code, out, err) = run_render(dir.path().to_str().unwrap());
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.starts_with("wintasks: "), "{err}");
    assert!(!err.contains("usage"), "{err}");
}

// ----------------------------------------------------- invalid arguments
// Spec: invalid arguments print usage and exit 2 (exit-code table).

#[test]
fn no_arguments_prints_usage_and_exits_two() {
    let (code, out, err) = run_cli(&[]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("usage: wintasks"), "{err}");
}

#[test]
fn unknown_command_prints_usage_and_exits_two() {
    let (code, out, err) = run_cli(&["bogus"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("usage: wintasks"), "{err}");
}

#[test]
fn option_without_value_prints_usage_and_exits_two() {
    let (code, out, err) = run_cli(&["render", "--path"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("usage: wintasks"), "{err}");
}

// ------------------------------------------------- apply-only options
// Spec common-options table: --mount / --dry-run / --prune apply to
// apply only; on render they are invalid arguments (usage, exit 2).

#[test]
fn render_rejects_mount() {
    let (code, out, err) = run_cli(&["render", "--mount", "M"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("usage: wintasks"), "{err}");
}

#[test]
fn render_rejects_dry_run() {
    let (code, out, err) = run_cli(&["render", "--dry-run"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("usage: wintasks"), "{err}");
}

#[test]
fn render_rejects_prune() {
    let (code, out, err) = run_cli(&["render", "--prune"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("usage: wintasks"), "{err}");
}

// -------------------------------------------------------- read failure
// Spec exit-code / error-display: file read failure is exit 1 with
// `wintasks: <path>: <io error content>` and no usage.

#[test]
fn missing_file_reports_read_error_and_exits_one() {
    let missing = "/nonexistent/wintasks-missing.yaml";
    let (code, out, err) = run_render(missing);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        err.starts_with(&format!("wintasks: {missing}: ")),
        "{err}"
    );
    assert!(!err.contains("usage"), "{err}");
}
