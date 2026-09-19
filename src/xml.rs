//! Task Scheduler XML generation via quick-xml.

use std::fmt;

use crate::def::{DESCRIPTION_MARKER, TaskDef};
use crate::hash::def_hash;
use crate::trigger::{ScheduleKind, TriggerXml, build_triggers, month_element, weekday_element};
use chrono::{NaiveDateTime, Timelike};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

pub const TASK_XML_NS: &str = "http://schemas.microsoft.com/windows/2004/02/mit/task";

pub struct TaskXml {
    pub xml: String,
    pub def_hash: String,
}

#[derive(Debug)]
pub struct XmlError {
    pub task: String,
    pub message: String,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "task `{}`: {}", self.task, self.message)
    }
}

/// Render one task definition into schtasks-compatible XML. `now` is the
/// local time of this run: it dates cron StartBoundaries and drives the
/// past-start correction.
pub fn render_task_xml(def: &TaskDef, now: NaiveDateTime) -> Result<TaskXml, XmlError> {
    let err = |message: String| XmlError {
        task: def.name.clone(),
        message,
    };
    let mut triggers = Vec::new();
    for trigger in &def.trigger {
        let built = build_triggers(trigger, now.date(), now).map_err(err)?;
        triggers.extend(built);
    }
    let hash = def_hash(&def.clone()).map_err(err)?;
    let setting = def.effective_setting().map_err(err)?;
    let xml = write_task_xml(
        def,
        &triggers,
        &hash,
        setting.run_level_highest,
        setting.logon_type,
    )
    .map_err(|e| err(e.to_string()))?;
    Ok(TaskXml {
        xml,
        def_hash: hash,
    })
}

type XmlWriter = Writer<Vec<u8>>;

fn write_task_xml(
    def: &TaskDef,
    triggers: &[TriggerXml],
    hash: &str,
    run_level_highest: bool,
    logon_type: crate::def::LogonType,
) -> Result<String, std::io::Error> {
    let mut w = Writer::new_with_indent(Vec::new(), b' ', 2);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut task = BytesStart::new("Task");
    task.push_attribute(("version", "1.2"));
    task.push_attribute(("xmlns", TASK_XML_NS));
    w.write_event(Event::Start(task))?;

    write_registration_info(&mut w, hash)?;
    write_triggers(&mut w, triggers)?;
    write_principal(&mut w, run_level_highest, logon_type)?;
    write_settings(&mut w)?;
    write_actions(&mut w, def)?;

    w.write_event(Event::End(BytesEnd::new("Task")))?;
    Ok(String::from_utf8(w.into_inner()).expect("XML writer produced invalid UTF-8"))
}

fn write_registration_info(w: &mut XmlWriter, hash: &str) -> Result<(), std::io::Error> {
    open(w, "RegistrationInfo")?;
    text_element(
        w,
        "Description",
        &format!("{DESCRIPTION_MARKER} def-hash: {hash}"),
    )?;
    close(w, "RegistrationInfo")
}

fn write_triggers(w: &mut XmlWriter, triggers: &[TriggerXml]) -> Result<(), std::io::Error> {
    open(w, "Triggers")?;
    for trigger in triggers {
        match trigger {
            TriggerXml::Calendar(c) => write_calendar_trigger(w, c)?,
            TriggerXml::Logon { delay } => {
                open(w, "LogonTrigger")?;
                text_element(w, "Delay", delay)?;
                close(w, "LogonTrigger")?;
            }
            TriggerXml::Boot { delay } => {
                open(w, "BootTrigger")?;
                text_element(w, "Delay", delay)?;
                close(w, "BootTrigger")?;
            }
            TriggerXml::Time { start_boundary } => {
                open(w, "TimeTrigger")?;
                text_element(w, "StartBoundary", &format_boundary(*start_boundary))?;
                close(w, "TimeTrigger")?;
            }
            TriggerXml::Registration => {
                w.write_event(Event::Empty(BytesStart::new("RegistrationTrigger")))?;
            }
        }
    }
    close(w, "Triggers")
}

fn write_calendar_trigger(
    w: &mut XmlWriter,
    c: &crate::trigger::CalendarTrigger,
) -> Result<(), std::io::Error> {
    open(w, "CalendarTrigger")?;
    text_element(w, "StartBoundary", &format_boundary(c.start_boundary))?;
    if let Some((interval, duration)) = &c.repetition {
        open(w, "Repetition")?;
        text_element(w, "Interval", interval)?;
        text_element(w, "Duration", duration)?;
        close(w, "Repetition")?;
    }
    write_schedule(w, &c.schedule)?;
    close(w, "CalendarTrigger")
}

fn write_schedule(w: &mut XmlWriter, schedule: &ScheduleKind) -> Result<(), std::io::Error> {
    match schedule {
        ScheduleKind::Daily { days_interval } => {
            open(w, "ScheduleByDay")?;
            text_element(w, "DaysInterval", &days_interval.to_string())?;
            close(w, "ScheduleByDay")?;
        }
        ScheduleKind::Weekly { days } => {
            open(w, "ScheduleByWeek")?;
            write_days_of_week(w, days)?;
            // No Weeks element: every week (like ScheduleByMonthDayOfWeek).
            close(w, "ScheduleByWeek")?;
        }
        ScheduleKind::Monthly { days, months } => {
            open(w, "ScheduleByMonth")?;
            open(w, "DaysOfMonth")?;
            for day in days {
                text_element(w, "Day", &day.to_string())?;
            }
            close(w, "DaysOfMonth")?;
            write_months(w, months)?;
            close(w, "ScheduleByMonth")?;
        }
        ScheduleKind::MonthlyDow { days, months } => {
            open(w, "ScheduleByMonthDayOfWeek")?;
            write_days_of_week(w, days)?;
            // No Weeks element: every week (spec).
            write_months(w, months)?;
            close(w, "ScheduleByMonthDayOfWeek")?;
        }
    }
    Ok(())
}

fn write_days_of_week(w: &mut XmlWriter, days: &[u32]) -> Result<(), std::io::Error> {
    open(w, "DaysOfWeek")?;
    for day in days {
        w.write_event(Event::Empty(BytesStart::new(weekday_element(*day))))?;
    }
    close(w, "DaysOfWeek")
}

fn write_months(w: &mut XmlWriter, months: &[u32]) -> Result<(), std::io::Error> {
    open(w, "Months")?;
    for month in months {
        w.write_event(Event::Empty(BytesStart::new(month_element(*month))))?;
    }
    close(w, "Months")
}

fn write_principal(
    w: &mut XmlWriter,
    run_level_highest: bool,
    logon_type: crate::def::LogonType,
) -> Result<(), std::io::Error> {
    open(w, "Principals")?;
    open(w, "Principal")?;
    text_element(
        w,
        "RunLevel",
        if run_level_highest {
            "Highest"
        } else {
            "LeastPrivilege"
        },
    )?;
    text_element(w, "LogonType", logon_type.xml_name())?;
    close(w, "Principal")?;
    close(w, "Principals")
}

fn write_settings(w: &mut XmlWriter) -> Result<(), std::io::Error> {
    open(w, "Settings")?;
    // Missed starts should run as soon as possible, and battery state must
    // neither block start nor stop a running task.
    text_element(w, "StartWhenAvailable", "true")?;
    text_element(w, "DisallowStartIfOnBatteries", "false")?;
    text_element(w, "StopIfGoingOnBatteries", "false")?;
    close(w, "Settings")
}

fn write_actions(w: &mut XmlWriter, def: &TaskDef) -> Result<(), std::io::Error> {
    open(w, "Actions")?;
    for action in &def.action {
        open(w, "Exec")?;
        text_element(w, "Command", &action.command)?;
        if let Some(args) = &action.args {
            text_element(w, "Arguments", args)?;
        }
        if let Some(dir) = TaskDef::resolve_working_directory(action) {
            text_element(w, "WorkingDirectory", &dir)?;
        }
        close(w, "Exec")?;
    }
    close(w, "Actions")
}

/// Local time without timezone offset (spec: StartBoundary is local).
fn format_boundary(t: NaiveDateTime) -> String {
    let (h, m, s) = (t.hour(), t.minute(), t.second());
    format!("{}T{:02}:{:02}:{:02}", t.date().format("%Y-%m-%d"), h, m, s)
}

fn open(w: &mut XmlWriter, name: &str) -> Result<(), std::io::Error> {
    w.write_event(Event::Start(BytesStart::new(name)))?;
    Ok(())
}

fn close(w: &mut XmlWriter, name: &str) -> Result<(), std::io::Error> {
    w.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

fn text_element(w: &mut XmlWriter, name: &str, text: &str) -> Result<(), std::io::Error> {
    open(w, name)?;
    // BytesText::new escapes special characters.
    w.write_event(Event::Text(BytesText::new(text)))?;
    close(w, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::{ActionDef, TriggerDef, TriggerKind};

    fn now() -> NaiveDateTime {
        // 2026-01-15 00:00:00 local; earlier than every cron start below.
        chrono::NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    }

    fn def(name: &str, trigger_value: &str) -> TaskDef {
        TaskDef {
            name: name.to_string(),
            trigger: vec![TriggerDef {
                kind: TriggerKind::Cron,
                value: trigger_value.to_string(),
            }],
            action: vec![ActionDef {
                command: "cmd.exe".to_string(),
                args: Some("/c echo hello world".to_string()),
                working_directory: None,
            }],
            setting: None,
        }
    }

    #[test]
    fn daily_cron_snapshot() {
        let result = render_task_xml(&def("Hello", "00 09 * * *"), now()).unwrap();
        let expected = r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>managed-by: wintasks; def-hash: PLACEHOLDER</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2026-01-15T09:00:00</StartBoundary>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
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
      <Command>cmd.exe</Command>
      <Arguments>/c echo hello world</Arguments>
    </Exec>
  </Actions>
</Task>"#;
        assert_eq!(
            result.xml,
            expected.replace("PLACEHOLDER", &result.def_hash)
        );
    }

    #[test]
    fn repetition_snapshot() {
        let result = render_task_xml(&def("Hello", "*/15 9-17 * * *"), now()).unwrap();
        assert!(
            result
                .xml
                .contains("<StartBoundary>2026-01-15T09:00:00</StartBoundary>"),
            "{}",
            result.xml
        );
        assert!(result.xml.contains("<Repetition>"), "{}", result.xml);
        assert!(
            result.xml.contains("<Interval>PT15M</Interval>"),
            "{}",
            result.xml
        );
        assert!(
            result.xml.contains("<Duration>PT9H</Duration>"),
            "{}",
            result.xml
        );
        assert!(
            result.xml.contains("<DaysInterval>1</DaysInterval>"),
            "{}",
            result.xml
        );
    }

    #[test]
    fn weekly_snapshot() {
        let result = render_task_xml(&def("Hello", "0 12 * * MON,WED"), now()).unwrap();
        assert!(result.xml.contains("<ScheduleByWeek>"), "{}", result.xml);
        assert!(result.xml.contains("<Monday/>"), "{}", result.xml);
        assert!(result.xml.contains("<Wednesday/>"), "{}", result.xml);
        assert!(!result.xml.contains("<Weeks>"), "{}", result.xml);
    }

    #[test]
    fn monthly_and_monthly_dow_snapshot() {
        let result = render_task_xml(&def("Hello", "0 12 1 JAN MON"), now()).unwrap();
        assert!(
            result.xml.contains("<ScheduleByMonthDayOfWeek>"),
            "{}",
            result.xml
        );
        assert!(result.xml.contains("<ScheduleByMonth>"), "{}", result.xml);
        assert!(result.xml.contains("<January/>"), "{}", result.xml);
        assert!(result.xml.contains("<Day>1</Day>"), "{}", result.xml);
    }

    #[test]
    fn multiple_triggers_and_actions_snapshot() {
        let task = TaskDef {
            name: "Multi".to_string(),
            trigger: vec![
                TriggerDef {
                    kind: TriggerKind::Cron,
                    value: "00 09 * * *".to_string(),
                },
                TriggerDef {
                    kind: TriggerKind::Startup,
                    value: "01:00".to_string(),
                },
                TriggerDef {
                    kind: TriggerKind::Boot,
                    value: "00:30".to_string(),
                },
                TriggerDef {
                    kind: TriggerKind::Once,
                    value: "2030-01-01 07:45:30".to_string(),
                },
                TriggerDef {
                    kind: TriggerKind::Now,
                    value: "dummy".to_string(),
                },
            ],
            action: vec![
                ActionDef {
                    command: "C:\\Tools\\a.exe".to_string(),
                    args: None,
                    working_directory: None,
                },
                ActionDef {
                    command: "b.exe".to_string(),
                    args: None,
                    working_directory: None,
                },
            ],
            setting: None,
        };
        let result = render_task_xml(&task, now()).unwrap();
        let xml = &result.xml;
        assert!(xml.contains("<LogonTrigger>"), "{xml}");
        assert!(xml.contains("<Delay>PT1H</Delay>"), "{xml}");
        assert!(xml.contains("<BootTrigger>"), "{xml}");
        assert!(xml.contains("<Delay>PT30M</Delay>"), "{xml}");
        assert!(xml.contains("<TimeTrigger>"), "{xml}");
        assert!(
            xml.contains("<StartBoundary>2030-01-01T07:45:30</StartBoundary>"),
            "{xml}"
        );
        assert!(xml.contains("<RegistrationTrigger/>"), "{xml}");
        assert!(
            xml.contains("<WorkingDirectory>C:\\Tools</WorkingDirectory>"),
            "{xml}"
        );
        // b.exe has no parent directory: no WorkingDirectory element.
        assert_eq!(xml.matches("<WorkingDirectory>").count(), 1);
        // No args: no Arguments element for both actions.
        assert!(!xml.contains("<Arguments>"), "{xml}");
    }

    #[test]
    fn run_as_highest_and_s4u_snapshot() {
        let mut task = def("Admin", "00 09 * * *");
        task.setting = Some(crate::def::SettingDef {
            run_as: Some(crate::def::BoolOrString::Bool(true)),
            logon_type: Some(crate::def::LogonType::S4u),
        });
        let result = render_task_xml(&task, now()).unwrap();
        assert!(
            result.xml.contains("<RunLevel>Highest</RunLevel>"),
            "{}",
            result.xml
        );
        assert!(
            result.xml.contains("<LogonType>S4U</LogonType>"),
            "{}",
            result.xml
        );
    }

    #[test]
    fn escapes_special_characters() {
        let mut task = def("Escape", "00 09 * * *");
        task.action[0].args = Some("a < b & c".to_string());
        let result = render_task_xml(&task, now()).unwrap();
        assert!(result.xml.contains("a &lt; b &amp; c"), "{}", result.xml);
    }

    #[test]
    fn description_marker_and_hash_shape() {
        let result = render_task_xml(&def("Hello", "00 09 * * *"), now()).unwrap();
        assert!(result.xml.contains(&format!(
            "<Description>managed-by: wintasks; def-hash: {}</Description>",
            result.def_hash
        )));
        assert_eq!(result.def_hash.len(), 64);
    }
}
