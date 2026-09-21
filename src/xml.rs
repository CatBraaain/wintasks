//! Task Scheduler XML rendering and comparison.

use std::fmt;

use chrono::{NaiveDateTime, Timelike};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer, XmlVersion};

use crate::def::{LogonType, TaskDef};
use crate::trigger::{ScheduleKind, TriggerXml, build_triggers, month_element, weekday_element};

pub const TASK_XML_NS: &str = "http://schemas.microsoft.com/windows/2004/02/mit/task";

type XmlWriter = Writer<Vec<u8>>;

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

pub fn render_task_xml(definition: &TaskDef, now: NaiveDateTime) -> Result<String, XmlError> {
    let error = |message| XmlError {
        task: definition.name.clone(),
        message,
    };
    let mut triggers = Vec::new();
    for trigger in &definition.trigger {
        triggers.extend(build_triggers(trigger, now.date(), now).map_err(error)?);
    }
    let setting = definition.effective_setting().map_err(error)?;
    write_task_xml(
        definition,
        &triggers,
        setting.run_level_highest,
        setting.logon_type,
    )
    .map_err(|error| XmlError {
        task: definition.name.clone(),
        message: error.to_string(),
    })
}

fn write_task_xml(
    definition: &TaskDef,
    triggers: &[TriggerXml],
    run_level_highest: bool,
    logon_type: LogonType,
) -> Result<String, std::io::Error> {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut task = BytesStart::new("Task");
    task.push_attribute(("version", "1.2"));
    task.push_attribute(("xmlns", TASK_XML_NS));
    writer.write_event(Event::Start(task))?;

    writer.write_event(Event::Empty(BytesStart::new("RegistrationInfo")))?;
    write_triggers(&mut writer, triggers)?;
    write_principal(&mut writer, run_level_highest, logon_type)?;
    write_settings(&mut writer)?;
    write_actions(&mut writer, definition)?;

    writer.write_event(Event::End(BytesEnd::new("Task")))?;
    Ok(String::from_utf8(writer.into_inner()).expect("XML writer produced UTF-8"))
}

fn write_triggers(writer: &mut XmlWriter, triggers: &[TriggerXml]) -> Result<(), std::io::Error> {
    open(writer, "Triggers")?;
    for trigger in triggers {
        match trigger {
            TriggerXml::Calendar(trigger) => write_calendar_trigger(writer, trigger)?,
            TriggerXml::Logon { delay } => {
                open(writer, "LogonTrigger")?;
                text_element(writer, "Delay", delay)?;
                close(writer, "LogonTrigger")?;
            }
            TriggerXml::Boot { delay } => {
                open(writer, "BootTrigger")?;
                text_element(writer, "Delay", delay)?;
                close(writer, "BootTrigger")?;
            }
            TriggerXml::Time { start_boundary } => {
                open(writer, "TimeTrigger")?;
                text_element(writer, "StartBoundary", &format_boundary(*start_boundary))?;
                close(writer, "TimeTrigger")?;
            }
            TriggerXml::Registration => {
                writer.write_event(Event::Empty(BytesStart::new("RegistrationTrigger")))?;
            }
        }
    }
    close(writer, "Triggers")
}

fn write_calendar_trigger(
    writer: &mut XmlWriter,
    trigger: &crate::trigger::CalendarTrigger,
) -> Result<(), std::io::Error> {
    open(writer, "CalendarTrigger")?;
    text_element(
        writer,
        "StartBoundary",
        &format_boundary(trigger.start_boundary),
    )?;
    if let Some((interval, duration)) = &trigger.repetition {
        open(writer, "Repetition")?;
        text_element(writer, "Interval", interval)?;
        text_element(writer, "Duration", duration)?;
        close(writer, "Repetition")?;
    }
    write_schedule(writer, &trigger.schedule)?;
    close(writer, "CalendarTrigger")
}

fn write_schedule(writer: &mut XmlWriter, schedule: &ScheduleKind) -> Result<(), std::io::Error> {
    match schedule {
        ScheduleKind::Daily { days_interval } => {
            open(writer, "ScheduleByDay")?;
            text_element(writer, "DaysInterval", &days_interval.to_string())?;
            close(writer, "ScheduleByDay")?;
        }
        ScheduleKind::Weekly { days } => {
            open(writer, "ScheduleByWeek")?;
            write_days_of_week(writer, days)?;
            close(writer, "ScheduleByWeek")?;
        }
        ScheduleKind::Monthly { days, months } => {
            open(writer, "ScheduleByMonth")?;
            open(writer, "DaysOfMonth")?;
            for day in days {
                text_element(writer, "Day", &day.to_string())?;
            }
            close(writer, "DaysOfMonth")?;
            write_months(writer, months)?;
            close(writer, "ScheduleByMonth")?;
        }
        ScheduleKind::MonthlyDow { days, months } => {
            open(writer, "ScheduleByMonthDayOfWeek")?;
            write_days_of_week(writer, days)?;
            write_months(writer, months)?;
            close(writer, "ScheduleByMonthDayOfWeek")?;
        }
    }
    Ok(())
}

fn write_days_of_week(writer: &mut XmlWriter, days: &[u32]) -> Result<(), std::io::Error> {
    open(writer, "DaysOfWeek")?;
    for day in days {
        writer.write_event(Event::Empty(BytesStart::new(weekday_element(*day))))?;
    }
    close(writer, "DaysOfWeek")
}

fn write_months(writer: &mut XmlWriter, months: &[u32]) -> Result<(), std::io::Error> {
    open(writer, "Months")?;
    for month in months {
        writer.write_event(Event::Empty(BytesStart::new(month_element(*month))))?;
    }
    close(writer, "Months")
}

fn write_principal(
    writer: &mut XmlWriter,
    run_level_highest: bool,
    logon_type: LogonType,
) -> Result<(), std::io::Error> {
    open(writer, "Principals")?;
    open(writer, "Principal")?;
    text_element(
        writer,
        "RunLevel",
        if run_level_highest {
            "HighestAvailable"
        } else {
            "LeastPrivilege"
        },
    )?;
    text_element(writer, "LogonType", logon_type.xml_name())?;
    close(writer, "Principal")?;
    close(writer, "Principals")
}

fn write_settings(writer: &mut XmlWriter) -> Result<(), std::io::Error> {
    open(writer, "Settings")?;
    text_element(writer, "StartWhenAvailable", "true")?;
    text_element(writer, "DisallowStartIfOnBatteries", "false")?;
    text_element(writer, "StopIfGoingOnBatteries", "false")?;
    close(writer, "Settings")
}

fn write_actions(writer: &mut XmlWriter, definition: &TaskDef) -> Result<(), std::io::Error> {
    open(writer, "Actions")?;
    for action in &definition.action {
        open(writer, "Exec")?;
        text_element(writer, "Command", &action.command)?;
        if let Some(args) = &action.args {
            text_element(writer, "Arguments", args)?;
        }
        if let Some(directory) = TaskDef::resolve_working_directory(action) {
            text_element(writer, "WorkingDirectory", &directory)?;
        }
        close(writer, "Exec")?;
    }
    close(writer, "Actions")
}

fn format_boundary(time: NaiveDateTime) -> String {
    format!(
        "{}T{:02}:{:02}:{:02}",
        time.date().format("%Y-%m-%d"),
        time.hour(),
        time.minute(),
        time.second()
    )
}

fn open(writer: &mut XmlWriter, name: &str) -> Result<(), std::io::Error> {
    writer.write_event(Event::Start(BytesStart::new(name)))?;
    Ok(())
}

fn close(writer: &mut XmlWriter, name: &str) -> Result<(), std::io::Error> {
    writer.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

fn text_element(writer: &mut XmlWriter, name: &str, text: &str) -> Result<(), std::io::Error> {
    open(writer, name)?;
    writer.write_event(Event::Text(BytesText::new(text)))?;
    close(writer, name)
}

pub fn normalized_task_xml(xml: &str) -> Result<String, String> {
    if xml.is_empty() {
        return Ok(String::new());
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|error| error.to_string())?;

    let mut elements = Vec::new();
    let mut excluded_depth = 0;
    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(event) => {
                let name = local_name(event.name().as_ref());
                let excluded = is_excluded(elements.last(), &name);
                elements.push(name.clone());
                if excluded_depth > 0 {
                    excluded_depth += 1;
                } else if name == "RegistrationInfo" {
                    writer
                        .write_event(Event::Empty(sorted_start(&event)?))
                        .map_err(|error| error.to_string())?;
                    excluded_depth = 1;
                } else if excluded {
                    excluded_depth = 1;
                } else {
                    writer
                        .write_event(Event::Start(sorted_start(&event)?))
                        .map_err(|error| error.to_string())?;
                }
            }
            Event::Empty(event) if excluded_depth == 0 => {
                let name = local_name(event.name().as_ref());
                if !is_excluded(elements.last(), &name) {
                    writer
                        .write_event(Event::Empty(sorted_start(&event)?))
                        .map_err(|error| error.to_string())?;
                }
            }
            Event::End(event) => {
                if excluded_depth > 0 {
                    excluded_depth -= 1;
                } else {
                    writer
                        .write_event(Event::End(event.into_owned()))
                        .map_err(|error| error.to_string())?;
                }
                elements.pop();
            }
            Event::Text(event) if excluded_depth == 0 => {
                if !event.xml_content(XmlVersion::Explicit1_0).trim().is_empty() {
                    writer
                        .write_event(Event::Text(event.into_owned()))
                        .map_err(|error| error.to_string())?;
                }
            }
            Event::Decl(_) => {}
            Event::Eof => break,
            event if excluded_depth == 0 => {
                writer
                    .write_event(event.into_owned())
                    .map_err(|error| error.to_string())?;
            }
            _ => {}
        }
    }
    String::from_utf8(writer.into_inner()).map_err(|error| error.to_string())
}

fn is_excluded(parent: Option<&String>, name: &str) -> bool {
    match parent.map(String::as_str) {
        Some("RegistrationInfo") => true,
        Some("CalendarTrigger") => matches!(name, "StartBoundary" | "Enabled" | "EndBoundary"),
        Some("LogonTrigger" | "BootTrigger" | "TimeTrigger") => name == "Enabled",
        Some("Principal") => !matches!(name, "RunLevel" | "LogonType"),
        Some("Settings") => !matches!(
            name,
            "StartWhenAvailable" | "DisallowStartIfOnBatteries" | "StopIfGoingOnBatteries"
        ),
        _ => false,
    }
}

fn local_name(name: &str) -> String {
    name.rsplit_once(':')
        .map_or_else(|| name.to_string(), |(_, local)| local.to_string())
}

fn sorted_start(event: &BytesStart<'_>) -> Result<BytesStart<'static>, String> {
    let element_name = local_name(event.name().as_ref());
    let mut attributes = event
        .attributes()
        .map(|attribute| attribute.map_err(|error| error.to_string()))
        .filter(|attribute| {
            attribute.as_ref().is_ok_and(|attribute| {
                !(element_name == "Actions" && local_name(attribute.key.as_ref()) == "Context")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    attributes.sort_by(|left, right| left.key.as_ref().cmp(right.key.as_ref()));

    let mut sorted = BytesStart::new(event.name().as_ref().to_owned());
    for attribute in attributes {
        sorted.push_attribute((attribute.key.as_ref(), attribute.value.as_ref()));
    }
    Ok(sorted)
}

pub fn task_xml_diff(current: &str, desired: &str) -> Result<Option<String>, String> {
    let current = normalized_task_xml(current)?;
    let desired = normalized_task_xml(desired)?;
    if current == desired {
        return Ok(None);
    }

    let current_lines: Vec<_> = current.lines().collect();
    let desired_lines: Vec<_> = desired.lines().collect();
    let mut suffixes = vec![vec![0; desired_lines.len() + 1]; current_lines.len() + 1];
    for current_index in (0..current_lines.len()).rev() {
        for desired_index in (0..desired_lines.len()).rev() {
            suffixes[current_index][desired_index] =
                if current_lines[current_index] == desired_lines[desired_index] {
                    suffixes[current_index + 1][desired_index + 1] + 1
                } else {
                    suffixes[current_index + 1][desired_index]
                        .max(suffixes[current_index][desired_index + 1])
                };
        }
    }

    let mut diff = String::from("--- current\n+++ desired\n@@\n");
    let (mut current_index, mut desired_index) = (0, 0);
    while current_index < current_lines.len() || desired_index < desired_lines.len() {
        if current_index < current_lines.len()
            && desired_index < desired_lines.len()
            && current_lines[current_index] == desired_lines[desired_index]
        {
            current_index += 1;
            desired_index += 1;
        } else if desired_index == desired_lines.len()
            || (current_index < current_lines.len()
                && suffixes[current_index + 1][desired_index]
                    >= suffixes[current_index][desired_index + 1])
        {
            diff.push('-');
            diff.push_str(current_lines[current_index]);
            diff.push('\n');
            current_index += 1;
        } else {
            diff.push('+');
            diff.push_str(desired_lines[desired_index]);
            diff.push('\n');
            desired_index += 1;
        }
    }
    Ok(Some(diff))
}
