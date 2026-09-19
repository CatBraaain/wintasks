//! `wintasks apply`: declarative reconciliation via schtasks.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use chrono::NaiveDateTime;

use crate::def::{TaskDef, parse_defs};
use crate::schtasks::{Schtasks, extract_tasks};
use crate::xml::{render_task_xml, task_xml_diff};

pub struct ApplyOptions {
    pub mount: String,
    pub dry_run: bool,
    pub prune: bool,
}

/// Task Scheduler path: `<mount>\<first trigger type>\<name>`.
pub fn task_path(def: &TaskDef, mount: &str) -> String {
    let folder = def.trigger[0].kind.folder();
    format!("\\{mount}\\{folder}\\{}", def.name)
}

enum Action {
    Create,
    Update,
    NoChange,
}

struct PlannedTask {
    path: String,
    action: Action,
    desired_xml: String,
    current_xml: Option<String>,
}

/// Runs apply and returns the process exit code. Reports go to `out`
/// (stdout) and errors to `err` (stderr) so callers can capture them.
pub fn run_apply(
    yaml_text: &str,
    file: &str,
    opts: &ApplyOptions,
    sched: &mut dyn Schtasks,
    now: NaiveDateTime,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    // Step 1: parse and generate all XML first; any failure leaves the
    // system untouched (spec).
    let defs = match parse_defs(yaml_text, file) {
        Ok(defs) => defs,
        Err(e) => return fail(&e.to_string(), err),
    };
    let mut desired = Vec::new();
    for def in &defs {
        let task = match render_task_xml(def, now) {
            Ok(task) => task,
            Err(e) => return fail(&format!("{file}: XML generation failed for {e}"), err),
        };
        desired.push((task_path(def, &opts.mount), task));
    }

    // Step 2: list system tasks and pick the managed ones.
    let query = match sched.query() {
        Ok(xml) => xml,
        Err(e) => return fail(&e.0, err),
    };
    let summary = extract_tasks(&query);
    let mut managed = BTreeMap::new();
    for task in summary.managed {
        managed.insert(task.path.clone(), task);
    }
    let all_paths = summary.all_paths;

    // Step 3: classify.
    let mut plan = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (path, task) in &desired {
        let action = match (managed.get(path), all_paths.contains(path)) {
            (Some(existing), _) if existing.def_hash.as_ref() == Some(&task.def_hash) => {
                Action::NoChange
            }
            (Some(_), _) => Action::Update,
            (None, true) => {
                // Same-name task without the marker: never overwrite.
                errors.push(format!(
                    "error: {path}: a task with this name exists but is not managed by wintasks; not registered"
                ));
                continue;
            }
            (None, false) => Action::Create,
        };
        plan.push(PlannedTask {
            path: path.clone(),
            action,
            desired_xml: task.xml.clone(),
            current_xml: managed.get(path).map(|existing| existing.xml.clone()),
        });
    }

    // Step 4: prune targets.
    let desired_paths: BTreeSet<&String> = desired.iter().map(|(p, _)| p).collect();
    let prune_targets = if opts.prune {
        managed
            .values()
            .filter(|task| !desired_paths.contains(&task.path))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    if opts.dry_run {
        for task in &plan {
            let _ = writeln!(out, "{} {}", label(&task.action), task.path);
            if matches!(&task.action, Action::Update)
                && let Err(message) = write_dry_run_diff(
                    task.current_xml
                        .as_deref()
                        .expect("updates have current XML"),
                    &task.desired_xml,
                    out,
                )
            {
                return fail(&message, err);
            }
        }
        for task in &prune_targets {
            let _ = writeln!(out, "delete {}", task.path);
            if let Err(message) = write_dry_run_diff(&task.xml, "", out) {
                return fail(&message, err);
            }
        }
        // A plan that would fail on a real apply (unmanaged same-name
        // collisions) must not exit 0.
        return finish(&errors, err);
    }

    // Step 5: execute creates/updates, then prunes; failures do not stop
    // the loop and are reported at the end (spec).
    for task in &plan {
        match &task.action {
            Action::NoChange => {
                let _ = writeln!(out, "no-change {}", task.path);
            }
            Action::Create | Action::Update => match sched.create(&task.path, &task.desired_xml) {
                Ok(()) => {
                    let _ = writeln!(out, "{} {}", label(&task.action), task.path);
                }
                Err(e) => errors.push(format!("error: {}", e.0)),
            },
        }
    }
    for task in &prune_targets {
        match sched.delete(&task.path) {
            Ok(()) => {
                let _ = writeln!(out, "delete {}", task.path);
            }
            Err(e) => errors.push(format!("error: {}", e.0)),
        }
    }

    finish(&errors, err)
}

fn write_dry_run_diff(current: &str, desired: &str, out: &mut dyn Write) -> Result<(), String> {
    match task_xml_diff(current, desired)? {
        Some(diff) => {
            let _ = write!(out, "{diff}");
        }
        None => {
            let _ = writeln!(out, "  (no diff)");
        }
    }
    Ok(())
}

fn label(action: &Action) -> &'static str {
    match action {
        Action::Create => "create",
        Action::Update => "update",
        Action::NoChange => "no-change",
    }
}

/// Drains accumulated errors to stderr and returns the exit code.
fn finish(errors: &[String], err: &mut dyn Write) -> i32 {
    if errors.is_empty() {
        return 0;
    }
    for error in errors {
        let _ = writeln!(err, "{error}");
    }
    1
}

fn fail(message: &str, err: &mut dyn Write) -> i32 {
    let _ = writeln!(err, "wintasks: {message}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schtasks::FakeSchtasks;
    use crate::xml::render_task_xml;

    fn now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    }

    fn opts() -> ApplyOptions {
        ApplyOptions {
            mount: "WinTasks".to_string(),
            dry_run: false,
            prune: false,
        }
    }

    const ONE_TASK: &str = "\
- name: Hello
  trigger: { type: cron, value: \"00 09 * * *\" }
  action: { command: cmd.exe, args: /c echo hi }
";

    const TWO_TASKS: &str = "\
- name: Hello
  trigger: { type: cron, value: \"00 09 * * *\" }
  action: { command: cmd.exe, args: /c echo hi }
- name: Bye
  trigger: { type: startup, value: \"01:00\" }
  action: { command: cmd.exe, args: /c echo bye }
";

    fn hash_of(yaml: &str, name: &str) -> String {
        let defs = parse_defs(yaml, "wintasks.yaml").unwrap();
        let def = defs.iter().find(|d| d.name == name).unwrap();
        render_task_xml(def, now()).unwrap().def_hash
    }

    fn run(yaml: &str, opts: &ApplyOptions, fake: &mut FakeSchtasks) -> i32 {
        capture(yaml, opts, fake).code
    }

    /// Runs apply against byte-vector sinks so reports and errors are
    /// observable as strings.
    fn capture(yaml: &str, opts: &ApplyOptions, sched: &mut dyn Schtasks) -> Captured {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run_apply(
            yaml,
            "wintasks.yaml",
            opts,
            sched,
            now(),
            &mut out,
            &mut err,
        );
        Captured {
            code,
            out: String::from_utf8_lossy(&out).into_owned(),
            err: String::from_utf8_lossy(&err).into_owned(),
        }
    }

    struct Captured {
        code: i32,
        out: String,
        err: String,
    }

    #[test]
    fn dry_run_diff_reports_no_diff_for_equivalent_xml() {
        let current =
            "<Task><RegistrationInfo><Description>old</Description></RegistrationInfo></Task>";
        let desired =
            "<Task><RegistrationInfo><Description>new</Description></RegistrationInfo></Task>";
        let mut out = Vec::new();

        write_dry_run_diff(current, desired, &mut out).unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "  (no diff)\n");
    }

    #[test]
    fn classifies_create_update_no_change() {
        // Hello exists with the same hash; Bye with a different hash.
        let same = hash_of(ONE_TASK, "Hello");
        let mut fake = FakeSchtasks::new();
        fake.tasks
            .insert("\\WinTasks\\cron\\Hello".to_string(), Some(same));
        fake.tasks
            .insert("\\WinTasks\\startup\\Bye".to_string(), Some("0".repeat(64)));
        let code = run(TWO_TASKS, &opts(), &mut fake);
        assert_eq!(code, 0);
        // Hello untouched; Bye overwritten.
        assert_eq!(fake.create_calls, vec!["\\WinTasks\\startup\\Bye"]);
    }

    #[test]
    fn creates_when_system_is_empty() {
        let mut fake = FakeSchtasks::new();
        let code = run(TWO_TASKS, &opts(), &mut fake);
        assert_eq!(code, 0);
        assert_eq!(
            fake.create_calls,
            vec!["\\WinTasks\\cron\\Hello", "\\WinTasks\\startup\\Bye"]
        );
    }

    #[test]
    fn unreadable_hash_forces_update() {
        let mut fake = FakeSchtasks::new();
        fake.tasks
            .insert("\\WinTasks\\cron\\Hello".to_string(), None);
        let code = run(ONE_TASK, &opts(), &mut fake);
        assert_eq!(code, 0);
        assert_eq!(fake.create_calls, vec!["\\WinTasks\\cron\\Hello"]);
    }

    #[test]
    fn refuses_to_overwrite_unmanaged_same_name() {
        // Present in the query but without the marker: not managed, so the
        // task must not be registered.
        let raw_query = crate::schtasks::marker_free_task_xml("\\WinTasks\\cron\\Hello");
        let mut sched = QueryOnceSchtasks {
            raw: raw_query,
            inner: FakeSchtasks::new(),
        };
        let cap = capture(ONE_TASK, &opts(), &mut sched);
        assert_eq!(cap.code, 1);
        assert!(sched.inner.create_calls.is_empty());
    }

    #[test]
    fn unmanaged_same_name_skip_continues_with_remaining_tasks() {
        // Hello collides with an unmanaged task; Bye must still register
        // (spec: skip one task, keep processing the rest).
        let raw_query = crate::schtasks::marker_free_task_xml("\\WinTasks\\cron\\Hello");
        let mut sched = QueryOnceSchtasks {
            raw: raw_query,
            inner: FakeSchtasks::new(),
        };
        let cap = capture(TWO_TASKS, &opts(), &mut sched);
        assert_eq!(cap.code, 1);
        assert_eq!(sched.inner.create_calls, vec!["\\WinTasks\\startup\\Bye"]);
    }

    #[test]
    fn skip_of_unmanaged_same_name_is_reported_on_stderr() {
        let raw_query = crate::schtasks::marker_free_task_xml("\\WinTasks\\cron\\Hello");
        let mut sched = QueryOnceSchtasks {
            raw: raw_query,
            inner: FakeSchtasks::new(),
        };
        let cap = capture(TWO_TASKS, &opts(), &mut sched);
        assert!(
            cap.err.contains("\\WinTasks\\cron\\Hello") && cap.err.contains("not managed"),
            "{}",
            cap.err
        );
    }

    #[test]
    fn prune_deletes_managed_tasks_missing_from_yaml() {
        let mut no_prune_fake = FakeSchtasks::new();
        no_prune_fake
            .tasks
            .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
        let code = run(ONE_TASK, &opts(), &mut no_prune_fake);
        assert_eq!(code, 0); // without prune, Gone stays
        assert!(no_prune_fake.delete_calls.is_empty());

        let mut fake = FakeSchtasks::new();
        fake.tasks
            .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
        let mut o = opts();
        o.prune = true;
        let code = run(ONE_TASK, &o, &mut fake);
        assert_eq!(code, 0);
        assert_eq!(fake.delete_calls, vec!["\\WinTasks\\cron\\Gone"]);
    }

    #[test]
    fn prune_deletes_managed_tasks_outside_the_mount_folder() {
        // Marker matching ignores folders (spec): a managed task under
        // \Other is still a prune target.
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
    fn prune_never_deletes_unmanaged_tasks() {
        // Unmanaged tasks never appear in `managed`, so prune ignores them.
        let raw_query = crate::schtasks::marker_free_task_xml("\\Other\\Important");
        let mut sched = QueryOnceSchtasks {
            raw: raw_query,
            inner: FakeSchtasks::new(),
        };
        let mut o = opts();
        o.prune = true;
        let cap = capture(ONE_TASK, &o, &mut sched);
        assert_eq!(cap.code, 0);
        assert!(sched.inner.delete_calls.is_empty());
    }

    #[test]
    fn dry_run_changes_nothing_and_reports_plan() {
        let mut fake = FakeSchtasks::new();
        fake.tasks
            .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
        let mut o = opts();
        o.dry_run = true;
        o.prune = true;
        let code = run(ONE_TASK, &o, &mut fake);
        assert_eq!(code, 0);
        assert!(fake.create_calls.is_empty());
        assert!(fake.delete_calls.is_empty());
    }

    #[test]
    fn dry_run_reports_classification_and_prune_targets() {
        // Hello exists with the same hash (no-change); Bye is new
        // (create); Gone is managed but absent from the YAML (delete).
        let same = hash_of(ONE_TASK, "Hello");
        let mut fake = FakeSchtasks::new();
        fake.tasks
            .insert("\\WinTasks\\cron\\Hello".to_string(), Some(same));
        fake.tasks
            .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
        let mut o = opts();
        o.dry_run = true;
        o.prune = true;
        let cap = capture(TWO_TASKS, &o, &mut fake);
        assert_eq!(cap.code, 0);
        let reported: Vec<&str> = cap
            .out
            .lines()
            .filter(|line| {
                matches!(
                    line.split_once(' ').map(|(action, _)| action),
                    Some("create" | "update" | "no-change" | "delete")
                )
            })
            .collect();
        assert_eq!(
            reported,
            vec![
                "no-change \\WinTasks\\cron\\Hello",
                "create \\WinTasks\\startup\\Bye",
                "delete \\WinTasks\\cron\\Gone",
            ],
            "{reported:?}"
        );
    }

    #[test]
    fn dry_run_reports_unmanaged_collision_as_nonzero() {
        let raw_query = crate::schtasks::marker_free_task_xml("\\WinTasks\\cron\\Hello");
        let mut sched = QueryOnceSchtasks {
            raw: raw_query,
            inner: FakeSchtasks::new(),
        };
        let mut o = opts();
        o.dry_run = true;
        let cap = capture(ONE_TASK, &o, &mut sched);
        assert_eq!(cap.code, 1);
        assert!(sched.inner.create_calls.is_empty());
    }

    #[test]
    fn parse_error_exits_nonzero_without_scheduling() {
        let mut fake = FakeSchtasks::new();
        let cap = capture("not: [valid\n", &opts(), &mut fake);
        assert_eq!(cap.code, 1);
        assert!(fake.create_calls.is_empty());
    }

    #[test]
    fn xml_error_exits_nonzero_without_scheduling() {
        let yaml = "- name: Bad\n  trigger: { type: cron, value: \"not cron\" }\n  action: { command: c }\n";
        let mut fake = FakeSchtasks::new();
        let cap = capture(yaml, &opts(), &mut fake);
        assert_eq!(cap.code, 1);
        assert!(fake.create_calls.is_empty());
    }

    #[test]
    fn xml_error_report_includes_task_name() {
        // Spec: XML generation errors name the task.
        let yaml = "- name: Bad\n  trigger: { type: cron, value: \"not cron\" }\n  action: { command: c }\n";
        let mut fake = FakeSchtasks::new();
        let cap = capture(yaml, &opts(), &mut fake);
        assert!(cap.err.contains("Bad"), "{}", cap.err);
    }

    #[test]
    fn schtasks_failure_continues_and_exits_nonzero() {
        let mut fake = FakeSchtasks::new();
        fake.fail_create_paths = vec!["\\WinTasks\\cron\\Hello".to_string()];
        let code = run(TWO_TASKS, &opts(), &mut fake);
        assert_eq!(code, 1);
        // The second task still ran.
        assert_eq!(fake.create_calls, vec!["\\WinTasks\\startup\\Bye"]);
    }

    #[test]
    fn schtasks_failure_report_includes_task_name_and_stderr() {
        // Spec: schtasks failures name the task and carry schtasks's
        // stderr ("ERROR ACCESS DENIED" in the fake).
        let mut fake = FakeSchtasks::new();
        fake.fail_create_paths = vec!["\\WinTasks\\cron\\Hello".to_string()];
        let cap = capture(TWO_TASKS, &opts(), &mut fake);
        assert!(
            cap.err.contains("\\WinTasks\\cron\\Hello") && cap.err.contains("ERROR ACCESS DENIED"),
            "{}",
            cap.err
        );
    }

    #[test]
    fn apply_reports_each_task_action() {
        // Hello is managed with a different hash (update); Bye is new
        // (create); Gone is pruned (delete).
        let mut fake = FakeSchtasks::new();
        fake.tasks
            .insert("\\WinTasks\\cron\\Hello".to_string(), Some("0".repeat(64)));
        fake.tasks
            .insert("\\WinTasks\\cron\\Gone".to_string(), Some("0".repeat(64)));
        let mut o = opts();
        o.prune = true;
        let cap = capture(TWO_TASKS, &o, &mut fake);
        assert_eq!(cap.code, 0);
        let reported: Vec<&str> = cap.out.lines().collect();
        assert_eq!(
            reported,
            vec![
                "update \\WinTasks\\cron\\Hello",
                "create \\WinTasks\\startup\\Bye",
                "delete \\WinTasks\\cron\\Gone",
            ],
            "{reported:?}"
        );
    }

    #[test]
    fn task_path_uses_first_trigger_type() {
        let defs = parse_defs(TWO_TASKS, "wintasks.yaml").unwrap();
        assert_eq!(task_path(&defs[0], "WinTasks"), "\\WinTasks\\cron\\Hello");
        assert_eq!(task_path(&defs[1], "Mount"), "\\Mount\\startup\\Bye");
    }

    /// Serves a fixed query document regardless of inner state, to express
    /// unmanaged tasks that FakeSchtasks's map cannot represent.
    struct QueryOnceSchtasks {
        raw: String,
        inner: FakeSchtasks,
    }

    impl Schtasks for QueryOnceSchtasks {
        fn query(&mut self) -> Result<String, crate::schtasks::SchtasksError> {
            Ok(self.raw.clone())
        }
        fn create(&mut self, path: &str, xml: &str) -> Result<(), crate::schtasks::SchtasksError> {
            self.inner.create(path, xml)
        }
        fn delete(&mut self, path: &str) -> Result<(), crate::schtasks::SchtasksError> {
            self.inner.delete(path)
        }
    }
}
