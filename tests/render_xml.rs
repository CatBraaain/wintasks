mod common;

use chrono::NaiveDate;
use common::definitions;
use wintasks::def::parse_defs;
use wintasks::xml::{normalized_task_xml, render_task_xml};

fn now() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 9, 20)
        .unwrap()
        .and_hms_opt(8, 0, 0)
        .unwrap()
}

#[test]
fn renders_spec_xml_shape_without_management_metadata() {
    let yaml = definitions(
        "  - name: backup\n    trigger: { type: cron, value: '0 9 * * MON' }\n    action: { command: 'C:\\Tools\\backup.exe', args: --full, working_directory: 'C:\\Backup' }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    let xml = render_task_xml(&definitions.tasks[0], now()).unwrap();
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">"));
    assert!(xml.contains("  <RegistrationInfo/>\n  <Triggers>"));
    assert!(
        !xml.contains("Description") && !xml.contains("def-hash") && !xml.contains("managed-by")
    );
    assert!(xml.contains("<Monday/>"));
    assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
    assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
    assert!(xml.contains("<StartWhenAvailable>true</StartWhenAvailable>"));
    assert!(xml.contains("<WorkingDirectory>C:\\Backup</WorkingDirectory>"));
}

#[test]
fn renders_the_spec_example_as_the_exact_full_document() {
    let yaml = definitions(
        "  - name: backup\n    trigger:\n      type: cron\n      value: \"0 9 * * MON\"\n    action:\n      command: C:\\Tools\\backup.exe\n      args: --full\n      working_directory: C:\\Backup\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    let xml = render_task_xml(&definitions.tasks[0], now()).unwrap();
    let expected = r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo/>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2026-09-20T09:00:00</StartBoundary>
      <ScheduleByWeek>
        <DaysOfWeek>
          <Monday/>
        </DaysOfWeek>
      </ScheduleByWeek>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal>
      <RunLevel>LeastPrivilege</RunLevel>
      <LogonType>InteractiveToken</LogonType>
    </Principal>
  </Principals>
  <Settings>
    <StartWhenAvailable>true</StartWhenAvailable>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
  </Settings>
  <Actions>
    <Exec>
      <Command>C:\Tools\backup.exe</Command>
      <Arguments>--full</Arguments>
      <WorkingDirectory>C:\Backup</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#;
    let compact = |xml: &str| xml.split_whitespace().collect::<String>();
    assert_eq!(compact(&xml), compact(expected), "generated:\n{xml}");
}

#[test]
fn renders_all_trigger_types_and_actions() {
    let yaml = definitions(
        "  - name: many\n    trigger:\n      - { type: startup, value: '01:30' }\n      - { type: boot, value: '00:30' }\n      - { type: once, value: '2030-06-01' }\n      - { type: now, value: unused }\n    action:\n      - { command: cmd.exe }\n      - { command: 'C:\\Tools\\a.exe', args: --one }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    let output = render_task_xml(&definitions.tasks[0], now()).unwrap();
    assert!(output.contains("<Delay>PT1H30M</Delay>"));
    assert!(output.contains("<Delay>PT30M</Delay>"));
    assert!(output.contains("<StartBoundary>2030-06-01T00:00:00</StartBoundary>"));
    assert!(output.contains("<RegistrationTrigger/>"));
    assert_eq!(output.matches("<Exec>").count(), 2);
    assert!(!output.contains("<Arguments></Arguments>"));
    let trigger_positions = [
        output.find("<Delay>PT1H30M</Delay>").unwrap(),
        output.find("<Delay>PT30M</Delay>").unwrap(),
        output
            .find("<StartBoundary>2030-06-01T00:00:00</StartBoundary>")
            .unwrap(),
        output.find("<RegistrationTrigger/>").unwrap(),
    ];
    assert!(
        trigger_positions.windows(2).all(|pair| pair[0] < pair[1]),
        "triggers must keep YAML order: {output}"
    );
    let command_positions = [
        output.find("<Command>cmd.exe</Command>").unwrap(),
        output.find("<Command>C:\\Tools\\a.exe</Command>").unwrap(),
    ];
    assert!(
        command_positions[0] < command_positions[1],
        "actions must keep YAML order: {output}"
    );
}

#[test]
fn normalizing_ignores_only_description_and_calendar_boundaries() {
    let current = "<Task z=\"z\" a=\"a\"><RegistrationInfo><Description>old</Description></RegistrationInfo><Triggers><CalendarTrigger><StartBoundary>old</StartBoundary></CalendarTrigger></Triggers></Task>";
    let same = "<Task a=\"a\" z=\"z\"><RegistrationInfo><Description>new</Description></RegistrationInfo><Triggers><CalendarTrigger><StartBoundary>new</StartBoundary></CalendarTrigger></Triggers></Task>";
    assert_eq!(
        normalized_task_xml(current).unwrap(),
        normalized_task_xml(same).unwrap()
    );
}

#[test]
fn normalizing_ignores_scheduler_metadata_and_defaults() {
    let current = "<Task><RegistrationInfo><URI>old-uri</URI><Date>old</Date></RegistrationInfo><Principals><Principal><UserId>old</UserId><RunLevel>LeastPrivilege</RunLevel><LogonType>InteractiveToken</LogonType></Principal></Principals><Settings><Enabled>true</Enabled><StartWhenAvailable>true</StartWhenAvailable></Settings><Actions Context=\"Author\"><Exec><Command>cmd.exe</Command></Exec></Actions></Task>";
    let desired = "<Task><RegistrationInfo/><Principals><Principal><RunLevel>LeastPrivilege</RunLevel><LogonType>InteractiveToken</LogonType></Principal></Principals><Settings><StartWhenAvailable>true</StartWhenAvailable></Settings><Actions><Exec><Command>cmd.exe</Command></Exec></Actions></Task>";
    assert_eq!(
        normalized_task_xml(current).unwrap(),
        normalized_task_xml(desired).unwrap()
    );
}

#[test]
fn root_child_order_and_settings_are_exact() {
    let yaml = definitions(
        "  - name: admin\n    trigger: { type: now, value: x }\n    action: { command: cmd.exe }\n    setting: { run_as: true, logon_type: s4u }\n",
    );
    let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
    let xml = render_task_xml(&definitions.tasks[0], now()).unwrap();
    let positions = [
        xml.find("<RegistrationInfo/>").unwrap(),
        xml.find("<Triggers>").unwrap(),
        xml.find("<Principals>").unwrap(),
        xml.find("<Settings>").unwrap(),
        xml.find("<Actions>").unwrap(),
    ];
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
    assert!(xml.contains("<LogonType>S4U</LogonType>"));
    assert!(xml.contains("<StartWhenAvailable>true</StartWhenAvailable>"));
    assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
    assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
}

#[test]
fn documented_conversion_cases_match_expected_fragments() {
    let compact = |xml: &str| xml.split_whitespace().collect::<String>();
    let at = |hour, minute| {
        NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    };
    let trigger_cases = [
        (
            "startup",
            "01:30",
            now(),
            "<LogonTrigger><Delay>PT1H30M</Delay></LogonTrigger>",
        ),
        (
            "boot",
            "00:30",
            now(),
            "<BootTrigger><Delay>PT30M</Delay></BootTrigger>",
        ),
        (
            "once",
            "2030-06-01",
            now(),
            "<TimeTrigger><StartBoundary>2030-06-01T00:00:00</StartBoundary></TimeTrigger>",
        ),
        ("now", "x", now(), "<RegistrationTrigger/>"),
        (
            "cron",
            "0 11 * * *",
            at(10, 0),
            "<CalendarTrigger><StartBoundary>2026-01-15T11:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>",
        ),
        (
            "cron",
            "0 9 * * *",
            at(10, 0),
            "<CalendarTrigger><StartBoundary>2026-01-16T09:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>",
        ),
        (
            "cron",
            "*/15 9-17 * * *",
            at(10, 0),
            "<CalendarTrigger><StartBoundary>2026-01-16T09:00:00</StartBoundary><Repetition><Interval>PT15M</Interval><Duration>PT9H</Duration></Repetition><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>",
        ),
        (
            "cron",
            "0,30 9,21 * * *",
            at(10, 0),
            "<CalendarTrigger><StartBoundary>2026-01-16T09:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger><CalendarTrigger><StartBoundary>2026-01-16T09:30:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger><CalendarTrigger><StartBoundary>2026-01-15T21:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger><CalendarTrigger><StartBoundary>2026-01-15T21:30:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>",
        ),
    ];
    for (kind, value, time, expected) in trigger_cases {
        let yaml = definitions(&format!(
            "  - name: sample\n    trigger: {{ type: {kind}, value: '{value}' }}\n    action: {{ command: cmd.exe }}\n"
        ));
        let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
        let xml = render_task_xml(&definitions.tasks[0], time).unwrap();
        let triggers = xml
            .split_once("<Triggers>")
            .unwrap()
            .1
            .split_once("</Triggers>")
            .unwrap()
            .0;
        assert_eq!(
            compact(triggers),
            compact(expected),
            "{kind} {value}: {xml}"
        );
    }

    for (command, args, directory, expected) in [
        (
            "cmd.exe",
            None,
            None,
            "<Exec><Command>cmd.exe</Command></Exec>",
        ),
        (
            "C:\\Tools\\a.exe",
            Some("--one"),
            None,
            "<Exec><Command>C:\\Tools\\a.exe</Command><Arguments>--one</Arguments><WorkingDirectory>C:\\Tools</WorkingDirectory></Exec>",
        ),
        (
            "C:\\Tools\\a.exe",
            None,
            Some("C:\\Data"),
            "<Exec><Command>C:\\Tools\\a.exe</Command><WorkingDirectory>C:\\Data</WorkingDirectory></Exec>",
        ),
    ] {
        let args = args.map_or(String::new(), |value| format!(", args: {value}"));
        let directory = directory.map_or(String::new(), |value| {
            format!(", working_directory: '{value}'")
        });
        let yaml = definitions(&format!(
            "  - name: sample\n    trigger: {{ type: now, value: x }}\n    action: {{ command: '{command}'{args}{directory} }}\n"
        ));
        let definitions = parse_defs(&yaml, "wintasks.yaml", "WinTasks").unwrap();
        let xml = render_task_xml(&definitions.tasks[0], now()).unwrap();
        let actions = xml
            .split_once("<Actions>")
            .unwrap()
            .1
            .split_once("</Actions>")
            .unwrap()
            .0;
        assert_eq!(compact(actions), compact(expected), "{xml}");
    }
}
