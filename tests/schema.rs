mod common;

use common::definitions;
use wintasks::def::parse_defs;

#[test]
fn parses_root_sequence_with_mount_supplied_by_cli() {
    let yaml = definitions(
        "  - name: backup\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    assert_eq!(definitions.mount, "WinTasks");
    assert_eq!(definitions.tasks.len(), 1);
}

#[test]
fn accepts_empty_sequence_but_rejects_empty_or_null_documents() {
    assert!(parse_defs("[]\n", "wintasks.yaml", "WinTasks").is_ok());
    for yaml in ["", "~\n"] {
        assert!(
            parse_defs(yaml, "wintasks.yaml", "WinTasks").is_err(),
            "{yaml:?}"
        );
    }
}

#[test]
fn rejects_invalid_mounts_supplied_by_cli() {
    let yaml = definitions(
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c }\n",
    );
    for mount in ["", "\\", "Root\\", "\\Root"] {
        assert!(
            parse_defs(&yaml, "wintasks.yaml", mount).is_err(),
            "{mount:?}"
        );
    }
}

#[test]
fn rejects_root_mappings_and_duplicate_task_names() {
    assert!(parse_defs("tasks: []\n", "wintasks.yaml", "WinTasks").is_err());
    assert!(parse_defs("mount: WinTasks\ntasks: []\n", "wintasks.yaml", "WinTasks").is_err());
    let duplicate = definitions(
        "  - name: same\n    trigger: { type: now, value: x }\n    action: { command: c }\n  - name: same\n    trigger: { type: now, value: x }\n    action: { command: c }\n",
    );
    assert!(parse_defs(&duplicate, "wintasks.yaml", "WinTasks").is_err());
}

#[test]
fn resolves_actions_and_settings() {
    let yaml = definitions(
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: 'C:\\Tools\\app.exe' }\n    setting: { run_as: 'true', logon_type: s4u }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    let task = &definitions.tasks[0];
    assert_eq!(
        task.effective_setting().unwrap().logon_type.xml_name(),
        "S4U"
    );
    assert_eq!(
        wintasks::def::TaskDef::resolve_working_directory(&task.action[0]).as_deref(),
        Some("C:\\Tools")
    );

    for command in ["C:\\app.exe", "C:/app.exe"] {
        let action = wintasks::def::ActionDef {
            command: command.to_string(),
            args: None,
            working_directory: None,
        };
        assert_eq!(
            wintasks::def::TaskDef::resolve_working_directory(&action).as_deref(),
            Some("C:\\")
        );
    }
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
            parse_defs(&definitions(tasks), "wintasks.yaml", "WinTasks").is_err(),
            "{tasks}"
        );
    }
}

#[test]
fn rejects_empty_and_nested_task_names_before_sync() {
    for name in ["", "child\\\\grandchild", "child/grandchild"] {
        let yaml = definitions(&format!(
            "  - name: '{name}'\n    trigger: {{ type: now, value: x }}\n    action: {{ command: c }}\n"
        ));
        let error = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("wintasks.yaml: invalid task name `{name}`")
        );
    }
}

#[test]
fn rejects_case_insensitive_duplicate_task_paths() {
    let yaml = definitions(
        "  - name: Task\n    trigger: { type: now, value: x }\n    action: { command: c }\n  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c }\n",
    );
    let error = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap_err();
    assert_eq!(
        error.to_string(),
        "wintasks.yaml: duplicate task path `\\WinTasks\\now\\task`"
    );
}

#[test]
fn duplicate_task_name_error_names_file_and_backticked_name() {
    let yaml = definitions(
        "  - name: x\n    trigger: { type: now, value: x }\n    action: { command: c }\n  - name: x\n    trigger: { type: now, value: x }\n    action: { command: c }\n",
    );
    let error = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap_err();
    assert_eq!(
        error.to_string(),
        "wintasks.yaml: duplicate task name `x`",
        "{yaml}"
    );
}

#[test]
fn located_yaml_parse_errors_carry_line_and_column_prefix() {
    let yaml = "- name: task\n  trigger: [\n";
    let error = parse_defs(yaml, "wintasks.yaml", "WinTasks")
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("wintasks.yaml:3:"),
        "expected `wintasks.yaml:<line>:<col>: ` prefix for {yaml:?}: {error}"
    );
}

#[test]
fn rejects_unknown_logon_type() {
    let yaml = definitions(
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c }\n    setting: { logon_type: batch }\n",
    );
    assert!(parse_defs(&yaml, "wintasks.yaml", "WinTasks").is_err());
}

#[test]
fn validates_setting_boolean_values() {
    for value in ["true", "false", "'true'", "'false'"] {
        let yaml = definitions(&format!(
            "  - name: task\n    trigger: {{ type: now, value: x }}\n    action: {{ command: c }}\n    setting: {{ run_as: {value} }}\n"
        ));
        assert!(
            parse_defs(&yaml, "wintasks.yaml", "WinTasks").is_ok(),
            "{value}"
        );
    }
    let yaml = definitions(
        "  - name: task\n    trigger: { type: now, value: x }\n    action: { command: c }\n    setting: { run_as: yes }\n",
    );
    let task = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    assert!(task.tasks[0].effective_setting().is_err());
}
