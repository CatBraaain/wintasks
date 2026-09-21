mod common;

use common::{FIXED_NOW, definitions};
use wintasks::apply::{ApplyRequest, SyncOptions, run_apply, task_path};
use wintasks::def::parse_defs;
use wintasks::schtasks::{Schtasks, SchtasksError};
use wintasks::xml::render_task_xml;

const ONE_TASK: &str = "  - name: backup\n    trigger: { type: cron, value: '0 9 * * *' }\n    action: { command: cmd.exe, args: /c backup }\n";

struct FakeSchtasks {
    query_xml: String,
    creates: Vec<String>,
    create_xmls: Vec<String>,
    deletes: Vec<String>,
    fail_create: Option<String>,
    fail_delete: Option<String>,
}

impl Schtasks for FakeSchtasks {
    fn query(&mut self) -> Result<String, SchtasksError> {
        Ok(self.query_xml.clone())
    }

    fn create(&mut self, path: &str, xml: &str) -> Result<(), SchtasksError> {
        if self.fail_create.as_deref() == Some(path) {
            return Err(SchtasksError("denied".to_string()));
        }
        self.creates.push(path.to_string());
        self.create_xmls.push(xml.to_string());
        Ok(())
    }

    fn delete(&mut self, path: &str) -> Result<(), SchtasksError> {
        if self.fail_delete.as_deref() == Some(path) {
            return Err(SchtasksError("denied".to_string()));
        }
        self.deletes.push(path.to_string());
        Ok(())
    }
}

fn queried_task(path: &str, body: &str) -> String {
    format!(
        "<Task xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\"><RegistrationInfo><URI>{}</URI></RegistrationInfo>{body}</Task>",
        path.replace('\\', "/")
    )
}

fn run(yaml: &str, dry_run: bool, scheduler: &mut FakeSchtasks) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_apply(
        &ApplyRequest {
            yaml_text: yaml,
            file: "wintasks.yaml",
            mount: "WinTasks",
        },
        &SyncOptions { dry_run },
        scheduler,
        FIXED_NOW,
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn creates_desired_tasks_and_deletes_all_omitted_tasks_in_mount() {
    let yaml = definitions(ONE_TASK);
    let mut scheduler = FakeSchtasks {
        query_xml: format!(
            "{}{}{}",
            queried_task("\\WinTasks", ""),
            queried_task("\\WinTasks\\other\\obsolete", ""),
            queried_task("\\Outside\\keep", "")
        ),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, out, err) = run(&yaml, false, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert_eq!(scheduler.creates, ["\\WinTasks\\cron\\backup"]);
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    assert_eq!(
        scheduler.create_xmls[0],
        render_task_xml(&definitions.tasks[0], FIXED_NOW).unwrap()
    );
    assert_eq!(
        scheduler.deletes,
        ["\\WinTasks", "\\WinTasks\\other\\obsolete"]
    );
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        [
            "create \\WinTasks\\cron\\backup",
            "delete \\WinTasks",
            "delete \\WinTasks\\other\\obsolete"
        ]
    );
}

#[test]
fn updates_mount_task_regardless_of_description_and_dry_run_shows_diff() {
    let yaml = definitions(ONE_TASK);
    let mut scheduler = FakeSchtasks {
        query_xml: queried_task(
            "\\WinTasks\\cron\\backup",
            "<Actions><Exec><Command>old.exe</Command></Exec></Actions>",
        ),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, out, err) = run(&yaml, true, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert!(scheduler.creates.is_empty() && scheduler.deletes.is_empty());
    assert!(
        out.starts_with("update \\WinTasks\\cron\\backup\n--- current\n+++ desired\n@@\n"),
        "{out}"
    );
    assert!(out.contains("-      <Command>old.exe</Command>"), "{out}");
}

#[test]
fn update_replaces_existing_mount_task() {
    let yaml = definitions(ONE_TASK);
    let mut scheduler = FakeSchtasks {
        query_xml: queried_task(
            "\\WinTasks\\cron\\backup",
            "<Actions><Exec><Command>old.exe</Command></Exec></Actions>",
        ),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, out, err) = run(&yaml, false, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert_eq!(scheduler.creates, ["\\WinTasks\\cron\\backup"]);
    assert_eq!(out, "update \\WinTasks\\cron\\backup\n");
}

#[test]
fn query_metadata_and_case_differences_are_no_change() {
    let yaml = definitions(ONE_TASK);
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    let desired = render_task_xml(&definitions.tasks[0], FIXED_NOW).unwrap();
    let current = desired
        .replace(
            "<RegistrationInfo/>",
            "<RegistrationInfo><URI>\\wintasks\\cron\\backup</URI><Date>2026-01-15T10:00:00</Date></RegistrationInfo>",
        )
        .replace("<Principal>", "<Principal><UserId>S-1-5-18</UserId>")
        .replace("<Settings>", "<Settings><Enabled>true</Enabled>")
        .replace("<Actions>", "<Actions Context=\"Author\">");
    let mut scheduler = FakeSchtasks {
        query_xml: current,
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, out, err) = run(&yaml, true, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "No changes.\n");
    assert!(scheduler.creates.is_empty() && scheduler.deletes.is_empty());
}

fn mixed_sync_fixture() -> (String, FakeSchtasks) {
    let yaml = definitions(
        "  - name: fresh\n    trigger: { type: cron, value: '0 9 * * *' }\n    action: { command: cmd.exe, args: /c fresh }\n  - name: refresh\n    trigger: { type: cron, value: '0 9 * * *' }\n    action: { command: cmd.exe, args: /c refresh }\n",
    );
    let scheduler = FakeSchtasks {
        query_xml: format!(
            "{}{}",
            queried_task(
                "\\WinTasks\\cron\\refresh",
                "<Actions><Exec><Command>old.exe</Command></Exec></Actions>"
            ),
            queried_task("\\WinTasks\\obsolete", "")
        ),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    (yaml, scheduler)
}

#[test]
fn mixed_sync_reports_definition_order_then_delete_and_orders_scheduler_calls() {
    let (yaml, mut scheduler) = mixed_sync_fixture();
    let (code, out, err) = run(&yaml, false, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        [
            "create \\WinTasks\\cron\\fresh",
            "update \\WinTasks\\cron\\refresh",
            "delete \\WinTasks\\obsolete",
        ],
        "{out}"
    );
    assert_eq!(
        scheduler.creates,
        ["\\WinTasks\\cron\\fresh", "\\WinTasks\\cron\\refresh"],
        "create calls must follow definition order"
    );
    assert_eq!(scheduler.deletes, ["\\WinTasks\\obsolete"]);
}

#[test]
fn dry_run_reports_no_changes_when_nothing_needs_syncing() {
    let mut scheduler = FakeSchtasks {
        query_xml: String::new(),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, out, err) = run("[]\n", true, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "No changes.\n");
    assert!(scheduler.creates.is_empty() && scheduler.deletes.is_empty());
}

#[test]
fn wrapped_query_tasks_are_no_change() {
    let yaml = definitions(
        "  - name: startup\n    trigger: { type: now, value: x }\n    action: { command: startup.exe }\n  - name: SyncTime\n    trigger: { type: now, value: x }\n    action: { command: synctime.exe }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    let query_xml = definitions
        .tasks
        .iter()
        .enumerate()
        .map(|(index, definition)| {
            let path = task_path(definition, "WinTasks");
            let desired = render_task_xml(definition, FIXED_NOW).unwrap();
            let uri = if index == 0 {
                format!("<URI>{path}</URI>")
            } else {
                format!("<URI><![CDATA[{path}]]></URI>")
            };
            desired
                .replace(
                    "<RegistrationInfo/>",
                    &format!(
                        "<RegistrationInfo>{uri}<Date>2026-01-15T10:00:00</Date></RegistrationInfo>"
                    ),
                )
                .replace(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                    "<?xml version=\"1.0\" encoding=\"UTF-16\"?>",
                )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut scheduler = FakeSchtasks {
        query_xml: format!("<Tasks>{query_xml}</Tasks>"),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, out, err) = run(&yaml, true, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "No changes.\n");
    assert!(scheduler.creates.is_empty() && scheduler.deletes.is_empty());
}

#[test]
fn dry_run_diffs_update_and_delete_but_not_create() {
    let (yaml, mut scheduler) = mixed_sync_fixture();
    let (code, out, err) = run(&yaml, true, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert!(
        scheduler.creates.is_empty() && scheduler.deletes.is_empty(),
        "dry-run must not change the system"
    );
    assert!(
        out.starts_with(
            "create \\WinTasks\\cron\\fresh\nupdate \\WinTasks\\cron\\refresh\n--- current\n+++ desired\n@@\n"
        ),
        "create must have no diff and update must be followed by its diff: {out}"
    );
    assert!(out.contains("-      <Command>old.exe</Command>"), "{out}");
    assert!(
        out.contains("delete \\WinTasks\\obsolete\n--- current\n+++ desired\n@@\n"),
        "delete must be followed by its diff: {out}"
    );
}

#[test]
fn malformed_query_stops_at_that_document_but_keeps_prior_tasks() {
    let yaml = definitions(
        "  - name: wanted\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n",
    );
    let mut scheduler = FakeSchtasks {
        query_xml: format!(
            "{}<!broken>{}",
            queried_task("\\WinTasks\\gone", ""),
            queried_task("\\WinTasks\\ignored", "")
        ),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, _, err) = run(&yaml, false, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert_eq!(scheduler.deletes, ["\\WinTasks\\gone"]);
}

#[test]
fn scheduler_failures_continue_and_are_reported() {
    let yaml = definitions(
        "  - name: one\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n  - name: two\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n",
    );
    let mut scheduler = FakeSchtasks {
        query_xml: String::new(),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: Some("\\WinTasks\\now\\one".to_string()),
        fail_delete: None,
    };
    let (code, out, err) = run(&yaml, false, &mut scheduler);
    assert_eq!(code, 1);
    assert_eq!(scheduler.creates, ["\\WinTasks\\now\\two"]);
    assert_eq!(out, "create \\WinTasks\\now\\two\n");
    assert_eq!(
        err,
        "error: schtasks /Create /TN \\WinTasks\\now\\one failed: denied\n"
    );
}

#[test]
fn delete_failures_are_reported_after_desired_tasks_continue() {
    let yaml = definitions(ONE_TASK);
    let mut scheduler = FakeSchtasks {
        query_xml: queried_task("\\WinTasks\\obsolete", ""),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: Some("\\WinTasks\\obsolete".to_string()),
    };
    let (code, out, err) = run(&yaml, false, &mut scheduler);
    assert_eq!(code, 1);
    assert_eq!(scheduler.creates, ["\\WinTasks\\cron\\backup"]);
    assert_eq!(out, "create \\WinTasks\\cron\\backup\n");
    assert_eq!(
        err,
        "error: schtasks /Delete /TN \\WinTasks\\obsolete failed: denied\n"
    );
}

struct QueryFailure;

impl Schtasks for QueryFailure {
    fn query(&mut self) -> Result<String, SchtasksError> {
        Err(SchtasksError(
            "schtasks /Query /XML failed: denied".to_string(),
        ))
    }

    fn create(&mut self, _: &str, _: &str) -> Result<(), SchtasksError> {
        panic!("query failure must stop before create")
    }

    fn delete(&mut self, _: &str) -> Result<(), SchtasksError> {
        panic!("query failure must stop before delete")
    }
}

#[test]
fn query_failure_exits_immediately() {
    let yaml = definitions(ONE_TASK);
    let mut scheduler = QueryFailure;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_apply(
        &ApplyRequest {
            yaml_text: &yaml,
            file: "wintasks.yaml",
            mount: "WinTasks",
        },
        &SyncOptions { dry_run: false },
        &mut scheduler,
        FIXED_NOW,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "wintasks: schtasks /Query /XML failed: denied\n"
    );
}

struct PreflightFailure;

impl Schtasks for PreflightFailure {
    fn query(&mut self) -> Result<String, SchtasksError> {
        panic!("preflight failures must stop before schtasks /Query")
    }

    fn create(&mut self, _: &str, _: &str) -> Result<(), SchtasksError> {
        panic!("preflight failures must stop before schtasks /Create")
    }

    fn delete(&mut self, _: &str) -> Result<(), SchtasksError> {
        panic!("preflight failures must stop before schtasks /Delete")
    }
}

fn run_preflight(yaml: &str) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_apply(
        &ApplyRequest {
            yaml_text: yaml,
            file: "wintasks.yaml",
            mount: "WinTasks",
        },
        &SyncOptions { dry_run: false },
        &mut PreflightFailure,
        FIXED_NOW,
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn yaml_parse_failure_exits_one_before_query() {
    let (code, out, err) = run_preflight("- name: broken\n  trigger: [\n");
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(
        err.starts_with("wintasks: wintasks.yaml:3:"),
        "located parse errors must use `wintasks: <file>:<line>:<col>: `: {err}"
    );
}

#[test]
fn xml_generation_failure_exits_one_before_query() {
    let yaml = definitions(
        "  - name: backup\n    trigger: { type: cron, value: 'bad cron' }\n    action: { command: cmd.exe }\n",
    );
    let (code, out, err) = run_preflight(&yaml);
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert_eq!(
        err,
        "wintasks: wintasks.yaml: XML generation failed for task `backup`: invalid cron expression `bad cron`: expected 5 fields (minute hour day month weekday), got 2\n"
    );
}

#[test]
fn dry_run_delete_prints_diff() {
    let yaml = definitions(
        "  - name: backup\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n",
    );
    let mut scheduler = FakeSchtasks {
        query_xml: queried_task("\\WinTasks\\obsolete", "<Actions/>"),
        creates: Vec::new(),
        create_xmls: Vec::new(),
        deletes: Vec::new(),
        fail_create: None,
        fail_delete: None,
    };
    let (code, out, err) = run(&yaml, true, &mut scheduler);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("delete \\WinTasks\\obsolete\n--- current\n+++ desired\n@@\n"));
    assert!(scheduler.creates.is_empty() && scheduler.deletes.is_empty());
}

#[test]
fn decodes_utf16_and_utf8_query_output() {
    let text = "task";
    let mut little_endian = vec![0xFF, 0xFE];
    little_endian.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    let mut big_endian = vec![0xFE, 0xFF];
    big_endian.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    assert_eq!(wintasks::schtasks::decode(&little_endian), text);
    assert_eq!(wintasks::schtasks::decode(&big_endian), text);
    assert_eq!(wintasks::schtasks::decode(text.as_bytes()), text);
}
