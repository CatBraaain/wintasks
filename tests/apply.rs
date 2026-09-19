//! Integration tests for `wintasks apply`.
//!
//! Oracle: SPEC.md — the "apply" section (steps 1-5 and its bullets),
//! the apply rows of the exit-code table, and the apply rows of the
//! error-display section. One `#[test]` per spec table row / bullet.
//! render-side behavior is covered by other test files and is only
//! verified here where the spec says apply uses the same conversion.

mod common;

use common::FIXED_NOW;
use wintasks::apply::{ApplyOptions, run_apply};
use wintasks::def::parse_defs;
use wintasks::hash::def_hash;
use wintasks::render::render_output;
use wintasks::schtasks::{decode, FakeSchtasks, Schtasks, SchtasksError};
use wintasks::parse_apply;

// ---------------------------------------------------------------- helpers

/// `- name: Hello` (cron) — the base single-task desired state.
const ONE_TASK: &str = "\
- name: Hello
  trigger: { type: cron, value: \"00 09 * * *\" }
  action: { command: cmd.exe, args: /c echo hi }
";

/// Hello (cron) + Bye (startup), YAML order Hello first.
const TWO_TASKS: &str = "\
- name: Hello
  trigger: { type: cron, value: \"00 09 * * *\" }
  action: { command: cmd.exe, args: /c echo hi }
- name: Bye
  trigger: { type: startup, value: \"01:00\" }
  action: { command: cmd.exe, args: /c echo bye }
";

/// Bye / New / Hello, deliberately not alphabetical so YAML order stays
/// observable in report output.
const THREE_TASKS: &str = "\
- name: Bye
  trigger: { type: startup, value: \"01:00\" }
  action: { command: cmd.exe, args: /c echo bye }
- name: New
  trigger: { type: once, value: \"2026-02-01 09:00\" }
  action: { command: cmd.exe, args: /c echo new }
- name: Hello
  trigger: { type: cron, value: \"00 09 * * *\" }
  action: { command: cmd.exe, args: /c echo hi }
";

struct Captured {
    code: i32,
    out: String,
    err: String,
}

fn opts() -> ApplyOptions {
    ApplyOptions {
        mount: "WinTasks".to_string(),
        dry_run: false,
        prune: false,
    }
}

/// Runs apply against byte sinks with the pinned clock so output is
/// deterministic; system effects are observed through the fake schtasks.
fn capture(yaml: &str, opts: &ApplyOptions, sched: &mut dyn Schtasks) -> Captured {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_apply(
        yaml,
        "wintasks.yaml",
        opts,
        sched,
        FIXED_NOW,
        &mut out,
        &mut err,
    );
    Captured {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// The def-hash the implementation computes for one task of `yaml`.
/// The spec pins only "same definition -> same hash", not concrete
/// values, so the hash is derived, not hard-coded.
fn def_hash_of(yaml: &str, name: &str) -> String {
    let defs = parse_defs(yaml, "wintasks.yaml").expect("parse");
    let def = defs.iter().find(|d| d.name == name).expect("task");
    def_hash(def).expect("hash")
}

/// A query-response document carrying the managed marker.
fn managed_query_doc(path: &str) -> String {
    query_doc(path, &format!("managed-by: wintasks; def-hash: {}", "0".repeat(64)))
}

/// A query-response document whose Description lacks the marker.
fn unmanaged_query_doc(path: &str) -> String {
    query_doc(path, "some unrelated description")
}

fn query_doc(path: &str, description: &str) -> String {
    let uri = path.replace('\\', "/");
    format!(
        "<?xml version=\"1.0\"?><Task xmlns=\"{NS}\"><RegistrationInfo>\
         <Description>{description}</Description><URI>{uri}</URI>\
         </RegistrationInfo></Task>",
        NS = wintasks::xml::TASK_XML_NS,
    )
}

/// The XML documents embedded in render output, YAML order.
fn rendered_xml_documents(rendered: &str) -> Vec<String> {
    rendered
        .split("--- ")
        .skip(1)
        .map(|block| {
            block
                .split_once('\n')
                .expect("separator line")
                .1
                .trim_end()
                .to_string()
        })
        .collect()
}

/// Serves a failing `/Query` while delegating create/delete, because
/// `FakeSchtasks::query` cannot fail.
struct QueryFailingSchtasks {
    inner: FakeSchtasks,
}

impl Schtasks for QueryFailingSchtasks {
    fn query(&mut self) -> Result<String, SchtasksError> {
        Err(SchtasksError(
            "schtasks /Query /XML failed: ERROR: Access is denied.".to_string(),
        ))
    }
    fn create(&mut self, path: &str, xml: &str) -> Result<(), SchtasksError> {
        self.inner.create(path, xml)
    }
    fn delete(&mut self, path: &str) -> Result<(), SchtasksError> {
        self.inner.delete(path)
    }
}

// ----------------------------------------------------------------- step 1
// Spec: parse errors, duplicate names, and XML generation errors abort
// before any system change.

#[test]
fn parse_error_exits_nonzero_without_system_changes() {
    let yaml = "\
- name: x
  trigger: { type: cron, value: \"* * * * *\" }
  action: { command: c }
  extra: 1
";
    let mut fake = FakeSchtasks::new();
    let cap = capture(yaml, &opts(), &mut fake);
    assert_eq!(cap.code, 1);
    assert!(fake.create_calls.is_empty() && fake.delete_calls.is_empty());
    // Spec: `wintasks: <file>:<line>:<col>: <reason>` when the location
    // is known; serde_yaml_ng reports one for this input.
    let first = cap.err.lines().next().unwrap_or_default();
    let location = first
        .strip_prefix("wintasks: wintasks.yaml:")
        .unwrap_or_default();
    let shape_ok = location.split_once(": ").is_some_and(|(loc, _)| {
        loc.split(':')
            .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    });
    assert!(shape_ok, "expected file:line:col format, got: {first}");
}

#[test]
fn duplicate_task_name_exits_nonzero_without_system_changes() {
    let yaml = "\
- name: dup
  trigger: { type: now, value: v }
  action: { command: c }
- name: dup
  trigger: { type: now, value: v }
  action: { command: c }
";
    let mut fake = FakeSchtasks::new();
    let cap = capture(yaml, &opts(), &mut fake);
    assert_eq!(cap.code, 1);
    assert!(fake.create_calls.is_empty() && fake.delete_calls.is_empty());
    // Spec: `wintasks: <file>: duplicate task name `<name>``.
    assert_eq!(
        cap.err,
        "wintasks: wintasks.yaml: duplicate task name `dup`\n"
    );
}

#[test]
fn xml_generation_error_exits_nonzero_without_system_changes() {
    let yaml = "\
- name: Bad
  trigger: { type: cron, value: \"not cron\" }
  action: { command: c }
";
    let mut fake = FakeSchtasks::new();
    let cap = capture(yaml, &opts(), &mut fake);
    assert_eq!(cap.code, 1);
    assert!(fake.create_calls.is_empty() && fake.delete_calls.is_empty());
    // Spec: `wintasks: <path>: XML generation failed for task `<name>`: <reason>`.
    assert!(
        cap.err
            .starts_with("wintasks: wintasks.yaml: XML generation failed for task `Bad`: "),
        "{}",
        cap.err
    );
}

// ----------------------------------------------------------------- step 2
// Spec: managed tasks are those whose Description starts with the
// marker, matched regardless of folder; an unparsable query document
// stops reading there and apply continues with the tasks read so far.

#[test]
fn marker_matching_ignores_folders() {
    // A managed task under a folder outside --mount is still matched and
    // becomes a prune target.
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\Other\\cron\\Gone".to_string(), Some("0".repeat(64)));
    let mut o = opts();
    o.prune = true;
    let cap = capture(ONE_TASK, &o, &mut fake);
    assert_eq!(cap.code, 0);
    assert_eq!(fake.delete_calls, vec!["\\Other\\cron\\Gone"]);
}

#[test]
fn query_output_with_utf16_le_bom_is_decoded() {
    // Spec: /Query output with a UTF-16 LE BOM is decoded as UTF-16.
    let text = "タスク";
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(decode(&bytes), text);
}

#[test]
fn query_output_with_utf16_be_bom_is_decoded() {
    // Spec: /Query output with a UTF-16 BE BOM is decoded as UTF-16.
    let text = "abc";
    let mut bytes = vec![0xFE, 0xFF];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    assert_eq!(decode(&bytes), text);
}

#[test]
fn query_output_without_bom_is_read_as_utf8() {
    // Spec: /Query output without a BOM is read as UTF-8.
    assert_eq!(decode("abc".as_bytes()), "abc");
}

#[test]
fn unparsable_query_document_ignores_rest_and_continues() {
    // Alpha parses, the next document is malformed, Beta comes after it:
    // Beta is ignored and apply still finishes with the partial result.
    let mut fake = FakeSchtasks::new();
    fake.extra_query_xml = format!(
        "{}{}{}",
        managed_query_doc("\\WinTasks\\cron\\Alpha"),
        "<!BROKEN>",
        managed_query_doc("\\WinTasks\\cron\\Beta"),
    );
    let mut o = opts();
    o.prune = true;
    let cap = capture(ONE_TASK, &o, &mut fake);
    assert_eq!(cap.code, 0);
    assert_eq!(fake.delete_calls, vec!["\\WinTasks\\cron\\Alpha"]);
}

// ----------------------------------------------------------------- step 3
// Spec classification table: create / update / no-change, unmanaged
// same-name collisions skipped, unreadable def-hash treated as update.

#[test]
fn create_when_no_managed_same_name_task() {
    // Spec: no same-name managed task -> `create`.
    let mut fake = FakeSchtasks::new();
    let cap = capture(ONE_TASK, &opts(), &mut fake);
    assert_eq!(cap.code, 0);
    assert_eq!(fake.create_calls, vec!["\\WinTasks\\cron\\Hello"]);
}

#[test]
fn update_when_def_hash_differs() {
    // Spec: same name with a different def-hash -> `update` (overwrite
    // via /F, i.e. a create call).
    let mut fake = FakeSchtasks::new();
    fake.tasks.insert(
        "\\WinTasks\\cron\\Hello".to_string(),
        Some("0".repeat(64)),
    );
    let cap = capture(ONE_TASK, &opts(), &mut fake);
    assert_eq!(cap.code, 0);
    assert_eq!(fake.create_calls, vec!["\\WinTasks\\cron\\Hello"]);
}

#[test]
fn no_change_when_def_hash_matches() {
    // Spec: same name with the same def-hash -> `no-change`, nothing is
    // executed. The marker-carrying query task is what makes it managed.
    let same = def_hash_of(ONE_TASK, "Hello");
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\WinTasks\\cron\\Hello".to_string(), Some(same));
    let cap = capture(ONE_TASK, &opts(), &mut fake);
    assert_eq!(cap.code, 0);
    assert!(fake.create_calls.is_empty() && fake.delete_calls.is_empty());
}

#[test]
fn passes_same_xml_as_render_to_schtasks_create() {
    // Spec: "render and apply use the same conversion" — the XML handed
    // to schtasks /Create must equal render's output at the same clock.
    let mut fake = FakeSchtasks::new();
    let cap = capture(TWO_TASKS, &opts(), &mut fake);
    assert_eq!(cap.code, 0);
    let defs = parse_defs(TWO_TASKS, "wintasks.yaml").expect("parse");
    let rendered = render_output(&defs, FIXED_NOW).expect("render");
    assert_eq!(fake.create_xmls, rendered_xml_documents(&rendered));
}

#[test]
fn unmanaged_same_name_not_registered_but_continues() {
    // Spec: a same-name task without the marker is not managed; it is
    // not registered, reported as an error, and the rest continues.
    let mut fake = FakeSchtasks::new();
    fake.extra_query_xml = unmanaged_query_doc("\\WinTasks\\cron\\Hello");
    let cap = capture(TWO_TASKS, &opts(), &mut fake);
    assert_eq!(cap.code, 1);
    assert_eq!(fake.create_calls, vec!["\\WinTasks\\startup\\Bye"]);
    // Spec: `error: <task name>: a task with this name exists but is
    // not managed by wintasks; not registered`.
    assert_eq!(
        cap.err,
        "error: \\WinTasks\\cron\\Hello: a task with this name exists but is not managed by wintasks; not registered\n"
    );
}

#[test]
fn unreadable_def_hash_treated_as_update() {
    // Spec: when the def-hash cannot be read from the Description, the
    // task is treated as a mismatch and updated.
    let mut fake = FakeSchtasks::new();
    fake.tasks.insert("\\WinTasks\\cron\\Hello".to_string(), None);
    let cap = capture(ONE_TASK, &opts(), &mut fake);
    assert_eq!(cap.code, 0);
    assert_eq!(fake.create_calls, vec!["\\WinTasks\\cron\\Hello"]);
}

#[test]
fn def_hash_does_not_include_mount() {
    // Spec: the def-hash excludes name and mount. The same definition
    // under a different mount must produce the identical XML (and
    // therefore the identical Description hash).
    let mut first = FakeSchtasks::new();
    capture(ONE_TASK, &opts(), &mut first);
    let mut second = FakeSchtasks::new();
    let other_mount = ApplyOptions {
        mount: "Other".to_string(),
        ..opts()
    };
    capture(ONE_TASK, &other_mount, &mut second);

    assert_eq!(first.create_xmls.len(), 1);
    assert_eq!(first.create_xmls, second.create_xmls);
    assert_ne!(first.create_calls, second.create_calls);
}

// ----------------------------------------------------------------- step 4
// Spec: --prune deletes managed tasks missing from the definitions;
// unmanaged tasks are never targets.

#[test]
fn prune_deletes_managed_tasks_missing_from_definitions() {
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
    let mut o = opts();
    o.prune = true;
    let cap = capture(ONE_TASK, &o, &mut fake);
    assert_eq!(cap.code, 0);
    assert_eq!(fake.delete_calls, vec!["\\WinTasks\\cron\\Gone"]);
}

#[test]
fn prune_does_not_delete_unmanaged_tasks() {
    let mut fake = FakeSchtasks::new();
    fake.extra_query_xml = unmanaged_query_doc("\\Other\\Important");
    let mut o = opts();
    o.prune = true;
    let cap = capture(ONE_TASK, &o, &mut fake);
    assert_eq!(cap.code, 0);
    assert!(fake.delete_calls.is_empty());
}

// ----------------------------------------------------------------- step 5
// Spec: reports go to stdout as `<classification> <task name>`, one
// line per completed action.

#[test]
fn reports_each_completed_action_in_execution_order() {
    // Bye -> update, New -> create, Hello -> no-change, Gone -> delete:
    // reports appear in YAML order as each completes, deletes last.
    let same = def_hash_of(THREE_TASKS, "Hello");
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\WinTasks\\startup\\Bye".to_string(), Some("0".repeat(64)));
    fake.tasks
        .insert("\\WinTasks\\cron\\Hello".to_string(), Some(same));
    fake.tasks
        .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
    let mut o = opts();
    o.prune = true;
    let cap = capture(THREE_TASKS, &o, &mut fake);
    assert_eq!(cap.code, 0);
    let reported: Vec<&str> = cap.out.lines().collect();
    assert_eq!(
        reported,
        vec![
            "update \\WinTasks\\startup\\Bye",
            "create \\WinTasks\\once\\New",
            "no-change \\WinTasks\\cron\\Hello",
            "delete \\WinTasks\\cron\\Gone",
        ],
        "{reported:?}"
    );
}

// ---------------------------------------------------------------- dry-run
// Spec: --dry-run stops after step 2, shows classifications in YAML
// order and deletes in task-name order, changes nothing, and exits
// nonzero on collision errors.

#[test]
fn dry_run_shows_plan_in_yaml_order_then_deletes_in_name_order() {
    let same = def_hash_of(THREE_TASKS, "Hello");
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\WinTasks\\startup\\Bye".to_string(), Some("0".repeat(64)));
    fake.tasks
        .insert("\\WinTasks\\cron\\Hello".to_string(), Some(same));
    fake.tasks
        .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
    fake.tasks
        .insert("\\WinTasks\\boot\\Alpha".to_string(), Some("0".repeat(64)));
    let mut o = opts();
    o.dry_run = true;
    o.prune = true;
    let cap = capture(THREE_TASKS, &o, &mut fake);
    assert_eq!(cap.code, 0);
    let planned: Vec<&str> = cap.out.lines().collect();
    assert_eq!(
        planned,
        vec![
            "update \\WinTasks\\startup\\Bye",
            "create \\WinTasks\\once\\New",
            "no-change \\WinTasks\\cron\\Hello",
            "delete \\WinTasks\\boot\\Alpha",
            "delete \\WinTasks\\cron\\Gone",
        ],
        "{planned:?}"
    );
}

#[test]
fn dry_run_changes_nothing() {
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
    let mut o = opts();
    o.dry_run = true;
    o.prune = true;
    let cap = capture(ONE_TASK, &o, &mut fake);
    assert_eq!(cap.code, 0);
    assert!(fake.create_calls.is_empty() && fake.delete_calls.is_empty());
}

#[test]
fn dry_run_with_conflict_exits_nonzero() {
    let mut fake = FakeSchtasks::new();
    fake.extra_query_xml = unmanaged_query_doc("\\WinTasks\\cron\\Hello");
    let mut o = opts();
    o.dry_run = true;
    let cap = capture(ONE_TASK, &o, &mut fake);
    assert_eq!(cap.code, 1);
    assert!(fake.create_calls.is_empty() && fake.delete_calls.is_empty());
}

// -------------------------------------------------- schtasks call failures
// Spec: create/delete failures continue processing, report the failed
// task name and schtasks error at the end, and exit nonzero.

#[test]
fn create_failure_continues_then_reports_and_exits_nonzero() {
    let mut fake = FakeSchtasks::new();
    fake.fail_create_paths = vec!["\\WinTasks\\cron\\Hello".to_string()];
    let cap = capture(TWO_TASKS, &opts(), &mut fake);
    assert_eq!(cap.code, 1);
    // The remaining task still ran.
    assert_eq!(fake.create_calls, vec!["\\WinTasks\\startup\\Bye"]);
    // Spec: `error: schtasks /Create /TN <task name> failed: <schtasks stderr>`.
    assert_eq!(
        cap.err,
        "error: schtasks /Create /TN \\WinTasks\\cron\\Hello failed: ERROR ACCESS DENIED\n"
    );
}

#[test]
fn delete_failure_continues_then_reports_and_exits_nonzero() {
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
    fake.fail_delete_paths = vec!["\\WinTasks\\cron\\Gone".to_string()];
    let mut o = opts();
    o.prune = true;
    let cap = capture(ONE_TASK, &o, &mut fake);
    assert_eq!(cap.code, 1);
    // The definitions still applied before the failing delete.
    assert_eq!(fake.create_calls, vec!["\\WinTasks\\cron\\Hello"]);
    // Spec: `error: schtasks /Delete /TN <task name> failed: <schtasks stderr>`.
    assert_eq!(
        cap.err,
        "error: schtasks /Delete /TN \\WinTasks\\cron\\Gone failed: ERROR ACCESS DENIED\n"
    );
}

// ------------------------------------------------------------ /Query failure
// Spec: a /Query failure cannot be classified around, so apply exits
// immediately.

#[test]
fn query_failure_exits_immediately() {
    let mut sched = QueryFailingSchtasks {
        inner: FakeSchtasks::new(),
    };
    let cap = capture(ONE_TASK, &opts(), &mut sched);
    assert_eq!(cap.code, 1);
    assert!(
        sched.inner.create_calls.is_empty() && sched.inner.delete_calls.is_empty(),
        "no system change after /Query failure"
    );
    // Spec: `wintasks: schtasks /Query /XML failed: <schtasks stderr>`.
    assert_eq!(
        cap.err,
        "wintasks: schtasks /Query /XML failed: ERROR: Access is denied.\n"
    );
}

// ------------------------------------------------------------ exit codes
// Spec exit-code table: success 0 regardless of changes; usage errors 2.

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

#[test]
fn success_exits_zero_regardless_of_changes() {
    // With changes: everything is created.
    let mut fake = FakeSchtasks::new();
    let cap = capture(TWO_TASKS, &opts(), &mut fake);
    assert_eq!(cap.code, 0, "with changes");
    assert_eq!(fake.create_calls.len(), 2);

    // Without changes: the same hash means nothing to do.
    let same = def_hash_of(ONE_TASK, "Hello");
    let mut fake = FakeSchtasks::new();
    fake.tasks
        .insert("\\WinTasks\\cron\\Hello".to_string(), Some(same));
    let cap = capture(ONE_TASK, &opts(), &mut fake);
    assert_eq!(cap.code, 0, "without changes");
}

#[test]
fn default_options_match_spec_values() {
    // Spec defaults: --path wintasks.yaml, --mount WinTasks,
    // --dry-run off, --prune off.
    let cli = parse_apply(&[]).expect("parse apply");
    assert_eq!(cli.path, "wintasks.yaml");
    assert_eq!(cli.opts.mount, "WinTasks");
    assert!(!cli.opts.dry_run);
    assert!(!cli.opts.prune);
}

#[test]
fn custom_mount_is_used_for_task_names() {
    // Spec: the task path is `<mount>\\<first trigger type>\\<name>`;
    // --mount replaces the WinTasks default.
    let mut fake = FakeSchtasks::new();
    let custom = ApplyOptions {
        mount: "Custom".to_string(),
        ..opts()
    };
    let cap = capture(ONE_TASK, &custom, &mut fake);
    assert_eq!(cap.code, 0);
    assert_eq!(fake.create_calls, vec!["\\Custom\\cron\\Hello"]);
}

#[test]
fn apply_only_option_on_render_exits_two_with_usage() {
    // Spec: --prune applies to apply only; misuse is a usage error.
    let (code, out, err) = run_cli(&["render", "--prune"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.starts_with("wintasks: "), "{err}");
    assert!(err.contains("usage: wintasks"), "{err}");
}

#[test]
fn invalid_apply_argument_exits_two_with_usage() {
    // Spec: malformed options (here --mount without a value) exit 2.
    let (code, out, err) = run_cli(&["apply", "--mount"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.starts_with("wintasks: "), "{err}");
    assert!(err.contains("usage: wintasks"), "{err}");
}

// ---------------------------------------------------------- error display
// Spec: file read failure reports the path and io error, exit 1.

#[test]
fn yaml_file_read_failure_reports_path_and_exits_one() {
    // Read failure lives above run_apply (the CLI reads the file), so
    // this case runs through the real CLI entry point.
    let path = common::write_temp_yaml("apply-read-failure", ONE_TASK);
    std::fs::remove_file(&path).expect("remove temp YAML");
    let (code, out, err) = run_cli(&["apply", "--path", &path]);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    // Spec: `wintasks: <path>: <io error content>`.
    assert!(
        err.starts_with(&format!("wintasks: {path}: ")),
        "{}",
        err
    );
}
