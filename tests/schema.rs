//! Integration tests for the wintasks YAML schema and task naming.
//!
//! Oracle: SPEC.md — the "YAML スキーマ" section (file shape and the
//! task / trigger / action / setting key tables) and the "タスク名"
//! section. One `#[test]` per spec table row / bullet.

mod common;

use common::{FIXED_NOW, run_render, write_temp_yaml};
use wintasks::apply::{ApplyOptions, run_apply};
use wintasks::schtasks::FakeSchtasks;

// ---------------------------------------------------------------- helpers

/// A one-line task body (everything after `- name: <name>`) with a
/// now trigger, used as the base for key-level variations.
fn task(trigger: &str, action: &str) -> String {
    format!("- name: T\n  trigger: {trigger}\n  action: {action}\n")
}

/// Renders `yaml` through the CLI and returns (code, stdout, stderr).
fn render(yaml: &str, test_name: &str) -> (u8, String, String) {
    let path = write_temp_yaml(test_name, yaml);
    run_render(&path)
}

/// The default apply options (mount WinTasks, no dry-run, no prune).
fn apply_opts() -> ApplyOptions {
    ApplyOptions {
        mount: "WinTasks".to_string(),
        dry_run: false,
        prune: false,
    }
}

// ------------------------------------------------------------ file shape
// Spec: the file is a list of task definitions; an empty file or a
// null document is an error; an empty list is a valid zero-task state;
// unknown keys are errors.

#[test]
fn file_is_a_list_of_task_definitions() {
    let yaml = "\
- name: A
  trigger: { type: now, value: v }
  action: { command: cmd.exe }
- name: B
  trigger: { type: startup, value: \"01:00\" }
  action: { command: cmd.exe }
";
    let (code, out, err) = render(yaml, "schema-two-tasks");
    assert_eq!(code, 0, "{err}");
    let separators: Vec<&str> = out
        .lines()
        .filter(|line| line.starts_with("--- "))
        .collect();
    assert_eq!(separators, ["--- A ---", "--- B ---"], "{out}");
}

#[test]
fn empty_file_is_error() {
    // Spec: `wintasks: <file>: no task definitions found (file is
    // empty or null)`.
    let (code, out, err) = render("", "schema-empty-file");
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        err.contains("no task definitions found (file is empty or null)"),
        "{err}"
    );
}

#[test]
fn null_document_is_error() {
    let (code, out, err) = render("~\n", "schema-null-doc");
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        err.contains("no task definitions found (file is empty or null)"),
        "{err}"
    );
}

#[test]
fn empty_list_is_valid_zero_task_state() {
    let (code, out, err) = render("[]\n", "schema-empty-list");
    assert_eq!(code, 0, "{err}");
    assert!(out.is_empty());
    assert!(err.is_empty());
}

#[test]
fn unknown_keys_are_rejected_at_every_level() {
    // Spec: an unknown key anywhere (task / trigger / action / setting
    // definition) is an error.
    let cases = [
        format!("{}\n  extra: 1\n", task("{ type: now, value: v }", "{ command: c }")),
        task("{ type: now, value: v, extra: 1 }", "{ command: c }"),
        task("{ type: now, value: v }", "{ command: c, extra: 1 }"),
        format!(
            "{}  setting: {{ extra: 1 }}\n",
            task("{ type: now, value: v }", "{ command: c }")
        ),
    ];
    for yaml in cases {
        let (code, _, err) = render(&yaml, "schema-unknown-key");
        assert_eq!(code, 1, "{yaml}: {err}");
    }
}

// ------------------------------------------------------------ task keys
// Spec task table: name / trigger / action required.

#[test]
fn name_is_required() {
    let yaml = "- trigger: { type: now, value: v }\n  action: { command: c }\n";
    let (code, _, _) = render(yaml, "schema-no-name");
    assert_eq!(code, 1);
}

#[test]
fn duplicate_task_names_are_error() {
    // Spec: `wintasks: <file>: duplicate task name `<name>``.
    let yaml = format!(
        "{yaml0}- name: T\n  trigger: {{ type: now, value: v }}\n  action: {{ command: c }}\n",
        yaml0 = task("{ type: now, value: v }", "{ command: c }")
    );
    let (code, _, err) = render(&yaml, "schema-dup-name");
    assert_eq!(code, 1);
    assert!(err.contains("duplicate task name `T`"), "{err}");
}

#[test]
fn trigger_is_required() {
    let yaml = "- name: T\n  action: { command: c }\n";
    let (code, _, _) = render(yaml, "schema-no-trigger");
    assert_eq!(code, 1);
}

#[test]
fn action_is_required() {
    let yaml = "- name: T\n  trigger: { type: now, value: v }\n";
    let (code, _, _) = render(yaml, "schema-no-action");
    assert_eq!(code, 1);
}

// --------------------------------------------------------- trigger keys
// Spec trigger table: single definition or array; type one of five;
// value required (for `now` its content is unused).

#[test]
fn trigger_accepts_single_definition_or_array() {
    // Single: exactly one trigger element in the XML.
    let single = task("{ type: now, value: v }", "{ command: c }");
    let (code, out, err) = render(&single, "schema-single-trigger");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.matches("<RegistrationTrigger/>").count(), 1, "{out}");

    // Array: each definition becomes a trigger, in YAML order.
    let array = "\
- name: T
  trigger:
    - { type: now, value: v }
    - { type: startup, value: \"01:00\" }
  action: { command: c }
";
    let (code, out, err) = render(array, "schema-trigger-array");
    assert_eq!(code, 0, "{err}");
    let now_pos = out.find("<RegistrationTrigger/>").expect("registration");
    let logon_pos = out.find("<LogonTrigger>").expect("logon");
    assert!(now_pos < logon_pos, "triggers out of YAML order: {out}");
}

#[test]
fn trigger_type_must_be_one_of_five() {
    let yaml = task("{ type: daily, value: v }", "{ command: c }");
    let (code, _, _) = render(&yaml, "schema-bad-type");
    assert_eq!(code, 1);
}

#[test]
fn trigger_value_is_required() {
    let yaml = task("{ type: now }", "{ command: c }");
    let (code, _, _) = render(&yaml, "schema-no-value");
    assert_eq!(code, 1);
}

#[test]
fn now_value_content_is_unused() {
    // Spec: `now` requires `value` but its content is unused, so any
    // string renders the same RegistrationTrigger.
    for value in ["v", "whatever 123"] {
        let yaml = task(&format!("{{ type: now, value: \"{value}\" }}"), "{ command: c }");
        let (code, out, err) = render(&yaml, "schema-now-value");
        assert_eq!(code, 0, "{err}");
        assert_eq!(out.matches("<RegistrationTrigger/>").count(), 1, "{out}");
    }
}

// --------------------------------------------------------- action keys
// Spec action table: command required; args optional;
// working_directory defaults to command's parent directory and is
// left unset when the command has no parent.

#[test]
fn command_is_required() {
    let yaml = task("{ type: now, value: v }", "{}");
    let (code, _, _) = render(&yaml, "schema-no-command");
    assert_eq!(code, 1);
}

#[test]
fn action_accepts_single_definition_or_array() {
    // Single: one Exec; array: one Exec per action in YAML order.
    let array = "\
- name: T
  trigger: { type: now, value: v }
  action:
    - { command: a.exe }
    - { command: b.exe }
";
    let (code, out, err) = render(array, "schema-action-array");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.matches("<Exec>").count(), 2, "{out}");
    let a_pos = out.find("<Command>a.exe</Command>").expect("a");
    let b_pos = out.find("<Command>b.exe</Command>").expect("b");
    assert!(a_pos < b_pos, "actions out of YAML order: {out}");
}

#[test]
fn args_is_optional() {
    let yaml = task("{ type: now, value: v }", "{ command: c }");
    let (code, out, err) = render(&yaml, "schema-no-args");
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("<Arguments>"), "{out}");
}

#[test]
fn working_directory_defaults_to_command_parent_or_is_unset() {
    // Unspecified: the parent directory of command is used.
    let yaml = task(
        "{ type: now, value: v }",
        r#"{ command: 'C:\Tools\app.exe' }"#,
    );
    let (code, out, err) = render(&yaml, "schema-wd-parent");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains(r#"<WorkingDirectory>C:\Tools</WorkingDirectory>"#), "{out}");

    // No parent (bare file name): the element is not written.
    let yaml = task("{ type: now, value: v }", "{ command: cmd.exe }");
    let (code, out, err) = render(&yaml, "schema-wd-none");
    assert_eq!(code, 0, "{err}");
    assert!(!out.contains("<WorkingDirectory>"), "{out}");
}

// -------------------------------------------------------- setting keys
// Spec setting table: run_as boolean or "true"/"false" strings;
// logon_type interactive_token / s4u, defaulting to interactive_token.

#[test]
fn run_as_accepts_bool_and_true_false_strings() {
    let run_level_of = |setting: &str| -> (u8, String) {
        let yaml = format!(
            "{base}  setting: {setting}\n",
            base = task("{ type: now, value: v }", "{ command: c }")
        );
        let (code, out, err) = render(&yaml, "schema-run-as");
        assert_eq!(code, 0, "{err}");
        let level = out
            .lines()
            .find(|line| line.contains("<RunLevel>"))
            .expect("RunLevel")
            .to_string();
        (code, level)
    };
    assert_eq!(
        run_level_of("{ run_as: true }").1,
        "      <RunLevel>Highest</RunLevel>"
    );
    assert_eq!(
        run_level_of("{ run_as: false }").1,
        "      <RunLevel>LeastPrivilege</RunLevel>"
    );
    assert_eq!(
        run_level_of(r#"{ run_as: "true" }"#).1,
        "      <RunLevel>Highest</RunLevel>"
    );
    assert_eq!(
        run_level_of(r#"{ run_as: "false" }"#).1,
        "      <RunLevel>LeastPrivilege</RunLevel>"
    );
}

#[test]
fn logon_type_accepts_two_values_and_defaults_to_interactive_token() {
    let logon_type_of = |setting: &str| -> (u8, String) {
        let yaml = format!(
            "{base}  setting: {setting}\n",
            base = task("{ type: now, value: v }", "{ command: c }")
        );
        let (code, out, _) = render(&yaml, "schema-logon-type");
        let value = out
            .lines()
            .find(|line| line.contains("<LogonType>"))
            .map(|line| line.to_string())
            .unwrap_or_default();
        (code, value)
    };
    assert_eq!(
        logon_type_of("{ logon_type: interactive_token }").1,
        "      <LogonType>InteractiveToken</LogonType>"
    );
    assert_eq!(
        logon_type_of("{ logon_type: s4u }").1,
        "      <LogonType>S4U</LogonType>"
    );
    // Unspecified: interactive_token.
    assert_eq!(
        logon_type_of("{}").1,
        "      <LogonType>InteractiveToken</LogonType>"
    );
    // Any other value is an error.
    let (code, _) = logon_type_of("{ logon_type: password }");
    assert_eq!(code, 1);
}

// -------------------------------------------------------------- task name
// Spec: the task name is `<mount>\<first trigger type>\<name>`; with a
// trigger array the first element's type is used. Observable through
// the /TN path apply hands to schtasks.

#[test]
fn task_name_uses_first_trigger_type_under_mount() {
    let yaml = "\
- name: Multi
  trigger:
    - { type: startup, value: \"01:00\" }
    - { type: cron, value: \"00 09 * * *\" }
  action: { command: cmd.exe }
";
    let mut fake = FakeSchtasks::new();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_apply(
        yaml,
        "wintasks.yaml",
        &apply_opts(),
        &mut fake,
        FIXED_NOW,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert_eq!(fake.create_calls, vec!["\\WinTasks\\startup\\Multi"]);
}
