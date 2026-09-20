mod common;

use common::definitions;
use wintasks::def::parse_defs;

#[test]
fn parses_root_mapping_with_mount_and_tasks() {
    let yaml = definitions(
        "  - name: backup\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml").unwrap();
    assert_eq!(definitions.mount, "WinTasks");
    assert_eq!(definitions.tasks.len(), 1);
}

#[test]
fn rejects_empty_or_null_documents_and_invalid_mounts() {
    for yaml in [
        "",
        "~\n",
        "mount: \\\ntasks: []\n",
        "mount: Root\\\ntasks: []\n",
        "mount: \\Root\ntasks: []\n",
    ] {
        assert!(parse_defs(yaml, "wintasks.yaml").is_err(), "{yaml}");
    }
}

#[test]
fn accepts_empty_desired_state_but_rejects_unknown_and_duplicate_keys() {
    assert!(parse_defs("mount: WinTasks\ntasks: []\n", "wintasks.yaml").is_ok());
    assert!(parse_defs("mount: WinTasks\ntasks: []\nextra: true\n", "wintasks.yaml").is_err());
    let duplicate = definitions(
        "  - name: same\n    trigger: { type: now, value: x }\n    action: { command: c }\n  - name: same\n    trigger: { type: now, value: x }\n    action: { command: c }\n",
    );
    assert!(parse_defs(&duplicate, "wintasks.yaml").is_err());
}

#[test]
fn resolves_actions_and_settings() {
    let yaml = definitions(
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: 'C:\\Tools\\app.exe' }\n    setting: { run_as: 'true', logon_type: s4u }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml").unwrap();
    let task = &definitions.tasks[0];
    assert_eq!(
        task.effective_setting().unwrap().logon_type.xml_name(),
        "S4U"
    );
    assert_eq!(
        wintasks::def::TaskDef::resolve_working_directory(&task.action[0]).as_deref(),
        Some("C:\\Tools")
    );
}

#[test]
fn rejects_missing_and_unknown_nested_keys() {
    for tasks in [
        "  - name: task\n    action: { command: c }\n",
        "  - name: task\n    trigger: { type: now, value: x }\n",
        "  - name: task\n    trigger: { type: now }\n    action: { command: c }\n",
        "  - name: task\n    trigger: { type: now, value: x, extra: true }\n    action: { command: c }\n",
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c, extra: true }\n",
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c }\n    setting: { extra: true }\n",
    ] {
        assert!(
            parse_defs(&definitions(tasks), "wintasks.yaml").is_err(),
            "{tasks}"
        );
    }
}

#[test]
fn duplicate_task_name_error_names_file_and_backticked_name() {
    let yaml = definitions(
        "  - name: x\n    trigger: { type: now, value: x }\n    action: { command: c }\n  - name: x\n    trigger: { type: now, value: x }\n    action: { command: c }\n",
    );
    let error = parse_defs(&yaml, "wintasks.yaml").unwrap_err();
    assert_eq!(
        error.to_string(),
        "wintasks.yaml: duplicate task name `x`",
        "{yaml}"
    );
}

#[test]
fn root_mount_and_empty_document_errors_have_exact_messages() {
    let root_mount = parse_defs("mount: \\\ntasks: []\n", "wintasks.yaml").unwrap_err();
    assert_eq!(
        root_mount.to_string(),
        "wintasks.yaml: mount must be a non-root folder path"
    );
    for yaml in ["", "~\n"] {
        let empty = parse_defs(yaml, "wintasks.yaml").unwrap_err();
        assert_eq!(
            empty.to_string(),
            "wintasks.yaml: no task definitions found (file is empty or null)",
            "{yaml:?}"
        );
    }
}

#[test]
fn located_yaml_parse_errors_carry_line_and_column_prefix() {
    let yaml = "mount: WinTasks\ntasks: [\n";
    let error = parse_defs(yaml, "wintasks.yaml").unwrap_err().to_string();
    assert!(
        error.starts_with("wintasks.yaml:3:1: "),
        "expected `wintasks.yaml:<line>:<col>: ` prefix for {yaml:?}: {error}"
    );
}

#[test]
fn rejects_missing_root_keys_task_unknown_keys_and_invalid_logon_type() {
    for yaml in [
        "tasks: []\n".to_string(),
        "mount: WinTasks\n".to_string(),
        definitions(
            "  - name: task\n    note: extra\n    trigger: { type: now, value: x }\n    action: { command: c }\n",
        ),
        definitions(
            "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c }\n    setting: { logon_type: batch }\n",
        ),
    ] {
        assert!(parse_defs(&yaml, "wintasks.yaml").is_err(), "{yaml}");
    }
}

#[test]
fn validates_setting_boolean_values() {
    for value in ["true", "false", "'true'", "'false'"] {
        let yaml = definitions(&format!(
            "  - name: task\n    trigger: {{ type: now, value: x }}\n    action: {{ command: c }}\n    setting: {{ run_as: {value} }}\n"
        ));
        assert!(parse_defs(&yaml, "wintasks.yaml").is_ok(), "{value}");
    }
    let yaml = definitions(
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c }\n    setting: { run_as: yes }\n",
    );
    let task = parse_defs(&yaml, "wintasks.yaml").unwrap();
    assert!(task.tasks[0].effective_setting().is_err());
}
