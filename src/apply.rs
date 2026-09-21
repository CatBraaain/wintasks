//! Declarative reconciliation of a mounted Task Scheduler subtree.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use chrono::NaiveDateTime;

use crate::def::{Definitions, TaskDef, parse_defs};
use crate::schtasks::{ExistingTask, Schtasks, extract_tasks};
use crate::xml::{normalized_task_xml, render_task_xml, task_xml_diff};

pub struct SyncOptions {
    pub dry_run: bool,
}

pub struct ApplyRequest<'a> {
    pub yaml_text: &'a str,
    pub file: &'a str,
    pub mount: &'a str,
}

pub fn task_path(definition: &TaskDef, mount: &str) -> String {
    format!(
        "\\{mount}\\{}\\{}",
        definition.trigger[0].kind.folder(),
        definition.name
    )
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

pub fn run_apply(
    request: &ApplyRequest<'_>,
    options: &SyncOptions,
    scheduler: &mut dyn Schtasks,
    now: NaiveDateTime,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let definitions = match parse_defs(request.yaml_text, request.file, request.mount) {
        Ok(definitions) => definitions,
        Err(error) => return fail(&error.to_string(), err),
    };
    run_sync(
        &definitions,
        request.file,
        options,
        scheduler,
        now,
        out,
        err,
    )
}

pub fn run_sync(
    definitions: &Definitions,
    file: &str,
    options: &SyncOptions,
    scheduler: &mut dyn Schtasks,
    now: NaiveDateTime,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let desired = match desired_tasks(definitions, file, now) {
        Ok(tasks) => tasks,
        Err(message) => return fail(&message, err),
    };
    let query = match scheduler.query() {
        Ok(query) => query,
        Err(error) => return fail(&error.0, err),
    };
    let current: BTreeMap<_, _> = extract_tasks(&query)
        .into_iter()
        .filter(|task| is_in_mount(&task.path, &definitions.mount))
        .map(|task| (task_path_key(&task.path), task))
        .collect();

    let plan = match classify(&desired, &current) {
        Ok(plan) => plan,
        Err(message) => return fail(&message, err),
    };
    let desired_paths: BTreeSet<_> = desired
        .iter()
        .map(|(path, _)| task_path_key(path))
        .collect();
    let deletions: Vec<_> = current
        .values()
        .filter(|task| !desired_paths.contains(&task_path_key(&task.path)))
        .cloned()
        .collect();

    if options.dry_run {
        if let Err(message) = write_dry_run(&plan, &deletions, out) {
            return fail(&message, err);
        }
        return 0;
    }

    let mut errors = Vec::new();
    for task in &plan {
        match &task.action {
            Action::NoChange => {
                let _ = writeln!(out, "no-change {}", task.path);
            }
            Action::Create | Action::Update => {
                match scheduler.create(&task.path, &task.desired_xml) {
                    Ok(()) => {
                        if let Err(message) = write_task_report(task, out) {
                            return fail(&message, err);
                        }
                    }
                    Err(error) => errors.push(format!(
                        "error: schtasks /Create /TN {} failed: {}",
                        task.path, error.0
                    )),
                }
            }
        }
    }
    for task in &deletions {
        match scheduler.delete(&task.path) {
            Ok(()) => {
                if let Err(message) = write_delete_report(task, out) {
                    return fail(&message, err);
                }
            }
            Err(error) => errors.push(format!(
                "error: schtasks /Delete /TN {} failed: {}",
                task.path, error.0
            )),
        }
    }
    finish(&errors, err)
}

fn desired_tasks(
    definitions: &Definitions,
    file: &str,
    now: NaiveDateTime,
) -> Result<Vec<(String, String)>, String> {
    definitions
        .tasks
        .iter()
        .map(|definition| {
            render_task_xml(definition, now)
                .map(|xml| (task_path(definition, &definitions.mount), xml))
                .map_err(|error| format!("{file}: XML generation failed for {error}"))
        })
        .collect()
}

fn classify(
    desired: &[(String, String)],
    current: &BTreeMap<String, ExistingTask>,
) -> Result<Vec<PlannedTask>, String> {
    desired
        .iter()
        .map(|(path, desired_xml)| {
            let existing = current.get(&task_path_key(path));
            let action = match existing {
                None => Action::Create,
                Some(existing)
                    if normalized_task_xml(&existing.xml)? == normalized_task_xml(desired_xml)? =>
                {
                    Action::NoChange
                }
                Some(_) => Action::Update,
            };
            Ok(PlannedTask {
                path: path.clone(),
                action,
                desired_xml: desired_xml.clone(),
                current_xml: existing.map(|task| task.xml.clone()),
            })
        })
        .collect()
}

fn is_in_mount(path: &str, mount: &str) -> bool {
    let path = task_path_key(path);
    let folder = task_path_key(&format!("\\{mount}"));
    path == folder || path.starts_with(&format!("{folder}\\"))
}

fn task_path_key(path: &str) -> String {
    path.to_lowercase()
}

fn write_dry_run(
    plan: &[PlannedTask],
    deletions: &[ExistingTask],
    out: &mut dyn Write,
) -> Result<(), String> {
    for task in plan {
        write_task_report(task, out)?;
    }
    for task in deletions {
        write_delete_report(task, out)?;
    }
    Ok(())
}

fn write_task_report(task: &PlannedTask, out: &mut dyn Write) -> Result<(), String> {
    let _ = writeln!(out, "{} {}", label(&task.action), task.path);
    if let (Action::Update, Some(current_xml)) = (&task.action, &task.current_xml) {
        write_diff(current_xml, &task.desired_xml, out)?;
    }
    Ok(())
}

fn write_delete_report(task: &ExistingTask, out: &mut dyn Write) -> Result<(), String> {
    let _ = writeln!(out, "delete {}", task.path);
    write_diff(&task.xml, "", out)
}

fn write_diff(current: &str, desired: &str, out: &mut dyn Write) -> Result<(), String> {
    if let Some(diff) = task_xml_diff(current, desired)? {
        let _ = write!(out, "{diff}");
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

    #[test]
    fn reports_each_no_change_entry_in_a_dry_run() {
        let path = "\\WinTasks\\now\\task".to_string();
        let mut current = BTreeMap::new();
        current.insert(
            task_path_key(&path),
            ExistingTask {
                path: path.clone(),
                xml: "<Task><RegistrationInfo><Description>old</Description></RegistrationInfo></Task>"
                    .to_string(),
            },
        );
        let plan = classify(
            &[(
                path,
                "<Task><RegistrationInfo><Description>new</Description></RegistrationInfo></Task>"
                    .to_string(),
            )],
            &current,
        )
        .unwrap();
        assert!(matches!(plan[0].action, Action::NoChange));
        let mut output = Vec::new();
        write_dry_run(&plan, &[], &mut output).unwrap();
        assert_eq!(output, b"no-change \\WinTasks\\now\\task\n");
    }
}
