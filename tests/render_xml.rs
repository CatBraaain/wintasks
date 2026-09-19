//! Integration tests for XML generation and `render` output.
//!
//! Oracle: SPEC.md — the "XML 生成" section (format bullets and the
//! embedded example document), the "render の出力" section, and the
//! trigger-conversion table. One `#[test]` per spec bullet / table
//! row / example.

mod common;

#[path = "fixtures/spec_cases.rs"]
mod spec_cases;

use chrono::NaiveDateTime;
use common::{run_render, write_temp_yaml};

// ---------------------------------------------------------------- helpers

/// A `- name: T` task with a now trigger, the base for most tests.
const NOW_TASK: &str = "\
- name: T
  trigger: { type: now, value: v }
  action: { command: cmd.exe }
";

/// Renders `yaml` through the CLI; returns (code, stdout, stderr).
fn render(yaml: &str, test_name: &str) -> (u8, String, String) {
    let path = write_temp_yaml(test_name, yaml);
    run_render(&path)
}

/// Renders `yaml` through `render_output` with a pinned clock, i.e.
/// the same conversion path the CLI uses. For expectations that
/// depend on the run date (StartBoundary, past-start correction).
fn render_at(yaml: &str, now: NaiveDateTime) -> String {
    let defs = wintasks::def::parse_defs(yaml, "wintasks.yaml").expect("parse defs");
    wintasks::render::render_output(&defs, now).expect("render output")
}

/// A fixed local time.
fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.and_hms_opt(hour, minute, 0))
        .expect("valid datetime")
}

/// The `<Description>...</Description>` line of the first task.
fn description_of(out: &str) -> &str {
    out.lines()
        .find(|line| line.contains("<Description>"))
        .expect("Description line")
}

/// A task with `trigger` and `action` inline.
fn task(trigger: &str, action: &str) -> String {
    format!("- name: T\n  trigger: {trigger}\n  action: {action}\n")
}

// ------------------------------------------------------------ XML format
// Spec: declaration, root element, 2-space indent, `<X/>` empty
// elements, and the fixed child order under the root.

#[test]
fn declaration_is_xml_1_0_utf_8() {
    let (code, out, err) = render(NOW_TASK, "xml-declaration");
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("--- T ---\n<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
        "{out}"
    );
}

#[test]
fn root_element_has_version_and_task_namespace() {
    let (_, out, _) = render(NOW_TASK, "xml-root");
    assert!(
        out.contains('\n'),
        "{out}"
    );
    assert!(
        out.contains("\n<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">"),
        "{out}"
    );
}

#[test]
fn indent_is_two_spaces_and_never_tabs() {
    let (_, out, _) = render(NOW_TASK, "xml-indent");
    // Root children are indented 2 spaces, their children 4.
    assert!(out.contains("\n  <Triggers>\n    <RegistrationTrigger/>"), "{out}");
    assert!(!out.contains('\t'), "{out}");
}

#[test]
fn empty_elements_use_self_closing_form() {
    let (_, out, _) = render(NOW_TASK, "xml-empty-element");
    assert!(out.contains("<RegistrationTrigger/>"), "{out}");
    assert!(!out.contains("</RegistrationTrigger>"), "{out}");
}

#[test]
fn root_children_are_in_documented_order() {
    let (_, out, _) = render(NOW_TASK, "xml-child-order");
    let order = [
        "<RegistrationInfo>",
        "<Triggers>",
        "<Principals>",
        "<Settings>",
        "<Actions>",
    ];
    let positions: Vec<usize> = order
        .iter()
        .map(|tag| out.find(tag).unwrap_or_else(|| panic!("{tag} missing: {out}")))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{out}"
    );
}

// ------------------------------------------------------------ Description
// Spec: Description is `managed-by: wintasks; def-hash: <def-hash>`
// where the hash is a 64-hex-digit SHA-256 over the effective
// trigger/action/setting values.

#[test]
fn description_carries_marker_and_sixtyfour_hex_hash() {
    let (_, out, _) = render(NOW_TASK, "xml-hash-shape");
    let description = description_of(&out);
    let hash = description
        .split_once("def-hash: ")
        .and_then(|(_, rest)| rest.split_once("</Description>"))
        .map(|(hash, _)| hash)
        .expect("def-hash in Description");
    assert_eq!(hash.len(), 64, "{description}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "{description}"
    );
}

#[test]
fn hash_equal_for_implicit_and_explicit_working_directory() {
    // Spec: the hash uses effective values, so an unspecified
    // working_directory and the explicit parent directory match.
    let implicit = task(
        "{ type: now, value: v }",
        r#"{ command: 'C:\Tools\app.exe' }"#,
    );
    let explicit = task(
        "{ type: now, value: v }",
        r#"{ command: 'C:\Tools\app.exe', working_directory: 'C:\Tools' }"#,
    );
    let (_, a, _) = render(&implicit, "xml-hash-wd-implicit");
    let (_, b, _) = render(&explicit, "xml-hash-wd-explicit");
    assert_eq!(description_of(&a), description_of(&b));
}

#[test]
fn hash_equal_for_default_and_explicit_logon_type() {
    let unset = task("{ type: now, value: v }", "{ command: c }");
    let explicit = format!(
        "{base}  setting: {{ logon_type: interactive_token }}\n",
        base = task("{ type: now, value: v }", "{ command: c }")
    );
    let (_, a, _) = render(&unset, "xml-hash-logon-unset");
    let (_, b, _) = render(&explicit, "xml-hash-logon-explicit");
    assert_eq!(description_of(&a), description_of(&b));
}

#[test]
fn hash_stable_for_identical_definition() {
    // Spec: the same definition always hashes the same.
    let (_, a, _) = render(NOW_TASK, "xml-hash-stable-a");
    let (_, b, _) = render(NOW_TASK, "xml-hash-stable-b");
    assert_eq!(description_of(&a), description_of(&b));
}

#[test]
fn hash_changes_when_effective_value_changes() {
    // Spec: one changed character in an effective value changes the
    // hash. args feeds straight into the XML.
    let one = task("{ type: now, value: v }", "{ command: c, args: a }");
    let other = task("{ type: now, value: v }", "{ command: c, args: ab }");
    let (_, a, _) = render(&one, "xml-hash-change-a");
    let (_, b, _) = render(&other, "xml-hash-change-b");
    assert_ne!(description_of(&a), description_of(&b));
}

#[test]
fn hash_ignores_task_name() {
    // Spec: def-hash does not include the task name.
    let one = task("{ type: now, value: v }", "{ command: c }").replace("- name: T", "- name: One");
    let other =
        task("{ type: now, value: v }", "{ command: c }").replace("- name: T", "- name: Two");
    let (_, a, _) = render(&one, "xml-hash-name-one");
    let (_, b, _) = render(&other, "xml-hash-name-two");
    assert_eq!(description_of(&a), description_of(&b));
}

// -------------------------------------------------------------- Principal
// Spec: Principal carries RunLevel (Highest only for run_as: true)
// and LogonType reflecting the setting (s4u -> S4U,
// interactive_token -> InteractiveToken).

#[test]
fn run_level_is_highest_only_for_run_as_true() {
    let run_level_of = |setting_suffix: &str| -> String {
        let yaml = format!(
            "{base}{setting_suffix}",
            base = task("{ type: now, value: v }", "{ command: c }")
        );
        let (_, out, _) = render(&yaml, "xml-run-level");
        out.lines()
            .find(|line| line.contains("<RunLevel>"))
            .expect("RunLevel")
            .to_string()
    };
    assert_eq!(
        run_level_of("  setting: { run_as: true }\n"),
        "      <RunLevel>Highest</RunLevel>"
    );
    assert_eq!(
        run_level_of("  setting: { run_as: false }\n"),
        "      <RunLevel>LeastPrivilege</RunLevel>"
    );
    // Spec: "anything other than run_as: true" — including no setting.
    assert_eq!(
        run_level_of(""),
        "      <RunLevel>LeastPrivilege</RunLevel>"
    );
}

#[test]
fn logon_type_reflects_setting() {
    let logon_type_of = |setting_suffix: &str| -> String {
        let yaml = format!(
            "{base}{setting_suffix}",
            base = task("{ type: now, value: v }", "{ command: c }")
        );
        let (_, out, _) = render(&yaml, "xml-logon-reflect");
        out.lines()
            .find(|line| line.contains("<LogonType>"))
            .expect("LogonType")
            .to_string()
    };
    assert_eq!(
        logon_type_of("  setting: { logon_type: s4u }\n"),
        "      <LogonType>S4U</LogonType>"
    );
    assert_eq!(
        logon_type_of("  setting: { logon_type: interactive_token }\n"),
        "      <LogonType>InteractiveToken</LogonType>"
    );
}

// --------------------------------------------------------------- Settings
// Spec: the three fixed Settings values.

#[test]
fn settings_have_the_three_spec_values() {
    let (_, out, _) = render(NOW_TASK, "xml-settings");
    assert!(out.contains("<StartWhenAvailable>true</StartWhenAvailable>"), "{out}");
    assert!(
        out.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"),
        "{out}"
    );
    assert!(
        out.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"),
        "{out}"
    );
}

// ---------------------------------------------------------------- Actions
// Spec: one Exec per action definition in YAML order; Command from
// command; Arguments omitted when absent; WorkingDirectory omitted
// when unresolved.

#[test]
fn exec_elements_follow_action_yaml_order() {
    let yaml = "\
- name: T
  trigger: { type: now, value: v }
  action:
    - { command: b.exe }
    - { command: a.exe }
";
    let (code, out, err) = render(yaml, "xml-exec-order");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.matches("<Exec>").count(), 2, "{out}");
    let b = out.find("<Command>b.exe</Command>").expect("b");
    let a = out.find("<Command>a.exe</Command>").expect("a");
    assert!(b < a, "actions out of YAML order: {out}");
}

#[test]
fn arguments_element_omitted_when_no_args() {
    let yaml = task("{ type: now, value: v }", "{ command: c }");
    let (_, out, _) = render(&yaml, "xml-no-arguments");
    assert!(!out.contains("<Arguments>"), "{out}");
}

#[test]
fn working_directory_element_omitted_when_unresolved() {
    // A bare command name has no parent directory, so no element.
    let yaml = task("{ type: now, value: v }", "{ command: cmd.exe }");
    let (_, out, _) = render(&yaml, "xml-no-working-directory");
    assert!(!out.contains("<WorkingDirectory>"), "{out}");
}

// --------------------------------------------------------------- Triggers
// Spec: triggers appear in YAML order; StartBoundary is local time
// with no timezone offset.

#[test]
fn trigger_elements_follow_yaml_order() {
    let yaml = "\
- name: T
  trigger:
    - { type: now, value: v }
    - { type: startup, value: \"01:00\" }
    - { type: boot, value: \"00:30\" }
  action: { command: c }
";
    let (code, out, err) = render(yaml, "xml-trigger-order");
    assert_eq!(code, 0, "{err}");
    let positions = [
        out.find("<RegistrationTrigger/>").expect("registration"),
        out.find("<LogonTrigger>").expect("logon"),
        out.find("<BootTrigger>").expect("boot"),
    ];
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "triggers out of YAML order: {out}"
    );
}

#[test]
fn start_boundary_has_no_timezone_offset() {
    // Spec: StartBoundary is local time without an offset.
    let yaml = task(
        "{ type: once, value: \"2030-06-01 07:45:30\" }",
        "{ command: c }",
    );
    let (_, out, _) = render(&yaml, "xml-boundary-offset");
    assert!(
        out.contains("<StartBoundary>2030-06-01T07:45:30</StartBoundary>"),
        "{out}"
    );
}

// ------------------------------------------------------- spec case tables
// Spec: the `## 変換テストケース` tables in SPEC.md define conversion-
// level cases; a case passes when the rendered output contains the
// expected fragment after whitespace-only text between tags is removed
// on both sides. The data lives in SPEC.md and is extracted by
// `tests/fixtures/spec_cases.rs`.

#[test]
fn spec_case_tables_render_the_expected_fragments() {
    let spec_cases = spec_cases::cases();
    assert!(!spec_cases.is_empty(), "spec case tables must not be empty");
    for case in spec_cases {
        let now = at(case.now.0, case.now.1, case.now.2, case.now.3, case.now.4);
        let out = render_at(&case.yaml, now);
        assert!(
            spec_cases::compact(&out).contains(&spec_cases::compact(&case.expected)),
            "case `{}`\ninput:\n{}\nexpected fragment: {}",
            case.name,
            case.yaml,
            case.expected
        );
    }
}

#[test]
fn spec_example_renders_the_documented_document() {
    let example = spec_cases::example();
    let out = render_at(&example.yaml, at(2026, 9, 20, 8, 0));
    assert!(
        spec_cases::compact(&out).contains(&spec_cases::compact(&example.expected)),
        "input:\n{}\nexpected document: {}",
        example.yaml,
        example.expected
    );
}

// --------------------------------------------------------- render output
// Spec: per task a `--- <name> ---` separator line followed by the
// XML document, no blank lines between tasks, nothing for 0 tasks.

#[test]
fn output_is_separator_line_then_xml_document() {
    let (code, out, err) = render(NOW_TASK, "render-block");
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("--- T ---\n<?xml"), "{out}");
    assert!(out.ends_with("</Task>\n"), "{out}");
}

#[test]
fn no_blank_line_between_tasks() {
    let yaml = "\
- name: A
  trigger: { type: now, value: v }
  action: { command: c }
- name: B
  trigger: { type: startup, value: \"01:00\" }
  action: { command: c }
";
    let (code, out, err) = render(yaml, "render-no-blank");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.matches("--- ").count(), 2, "{out}");
    assert!(!out.contains("\n\n"), "{out}");
}

#[test]
fn zero_definitions_produce_no_output() {
    let (code, out, err) = render("[]\n", "render-zero");
    assert_eq!(code, 0, "{err}");
    assert!(out.is_empty());
}

// ------------------------------------------------------ trigger elements
// Spec trigger-conversion table: one row per type, plus the
// "value not matching the form is an error" sentence.

#[test]
fn cron_maps_to_calendar_trigger() {
    let yaml = task("{ type: cron, value: \"00 09 * * *\" }", "{ command: c }");
    let (code, out, err) = render(&yaml, "trig-cron");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.matches("<CalendarTrigger>").count(), 1, "{out}");
}

#[test]
fn startup_maps_to_logon_trigger_with_delay() {
    // Spec: HH:MM and HH:MM:SS both convert to an ISO 8601 duration.
    let hh_mm = task("{ type: startup, value: \"01:30\" }", "{ command: c }");
    let (_, out, _) = render(&hh_mm, "trig-startup-hhmm");
    assert!(out.contains("<LogonTrigger>"), "{out}");
    assert!(out.contains("<Delay>PT1H30M</Delay>"), "{out}");

    let hh_mm_ss = task("{ type: startup, value: \"00:00:30\" }", "{ command: c }");
    let (_, out, _) = render(&hh_mm_ss, "trig-startup-hhmmss");
    assert!(out.contains("<Delay>PT30S</Delay>"), "{out}");
}

#[test]
fn boot_maps_to_boot_trigger_with_delay() {
    let yaml = task("{ type: boot, value: \"00:30\" }", "{ command: c }");
    let (code, out, err) = render(&yaml, "trig-boot");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("<BootTrigger>"), "{out}");
    assert!(out.contains("<Delay>PT30M</Delay>"), "{out}");
}

#[test]
fn once_maps_to_time_trigger_with_zero_for_omitted_time() {
    let date_only = task("{ type: once, value: \"2030-06-01\" }", "{ command: c }");
    let (_, out, _) = render(&date_only, "trig-once-date");
    assert!(out.contains("<TimeTrigger>"), "{out}");
    assert!(
        out.contains("<StartBoundary>2030-06-01T00:00:00</StartBoundary>"),
        "{out}"
    );

    let minute = task("{ type: once, value: \"2030-06-01 07:45\" }", "{ command: c }");
    let (_, out, _) = render(&minute, "trig-once-minute");
    assert!(
        out.contains("<StartBoundary>2030-06-01T07:45:00</StartBoundary>"),
        "{out}"
    );

    let second = task("{ type: once, value: \"2030-06-01 07:45:30\" }", "{ command: c }");
    let (_, out, _) = render(&second, "trig-once-second");
    assert!(
        out.contains("<StartBoundary>2030-06-01T07:45:30</StartBoundary>"),
        "{out}"
    );
}

#[test]
fn now_maps_to_registration_trigger() {
    let (code, out, err) = render(NOW_TASK, "trig-now");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("<RegistrationTrigger/>"), "{out}");
}

#[test]
fn trigger_value_not_matching_the_form_is_error() {
    // Spec: a value that does not fit the type's form is an error
    // (exit 1, XML generation failed).
    let cases = [
        ("startup", "9:00"),   // not two digits
        ("boot", "0100"),      // not colon-separated
        ("once", "01-01-2000"), // not YYYY-MM-DD
    ];
    for (kind, value) in cases {
        let yaml = task(&format!("{{ type: {kind}, value: \"{value}\" }}"), "{ command: c }");
        let (code, out, err) = render(&yaml, "trig-invalid-value");
        assert_eq!(code, 1, "{kind} {value}: {err}");
        assert!(out.is_empty(), "{kind} {value}: {out}");
        assert!(
            err.contains("XML generation failed"),
            "{kind} {value}: {err}"
        );
    }
}

// Spec: Delay omits zero units, is PT0S when all zero, and accepts
// HH above 23.

#[test]
fn delay_omits_zero_units() {
    // Spec example: `00:30` -> `PT30M` (the zero hours unit is omitted).
    let yaml = task("{ type: startup, value: \"00:30\" }", "{ command: c }");
    let (_, out, _) = render(&yaml, "delay-zero-units");
    assert!(out.contains("<Delay>PT30M</Delay>"), "{out}");
}

#[test]
fn delay_all_zero_is_pt0s() {
    let yaml = task("{ type: startup, value: \"00:00\" }", "{ command: c }");
    let (_, out, _) = render(&yaml, "delay-pt0s");
    assert!(out.contains("<Delay>PT0S</Delay>"), "{out}");
}

#[test]
fn delay_accepts_hours_over_23() {
    // Spec example: `25:00` -> `PT25H`.
    let yaml = task("{ type: boot, value: \"25:00\" }", "{ command: c }");
    let (code, out, err) = render(&yaml, "delay-pt25h");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("<Delay>PT25H</Delay>"), "{out}");
}
