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

#[derive(Debug)]
struct XmlNode {
    name: String,
    attributes: Vec<(String, String)>,
    content: Vec<XmlContent>,
}

#[derive(Debug)]
enum XmlContent {
    Element(XmlNode),
    Text(String),
    CData(String),
    Reference(String),
}

pub fn normalized_task_xml(xml: &str) -> Result<String, String> {
    if xml.is_empty() {
        return Ok(String::new());
    }

    let root = normalize_node(parse_xml(xml)?)?;
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|error| error.to_string())?;
    write_normalized_node(&mut writer, &root)?;
    String::from_utf8(writer.into_inner()).map_err(|error| error.to_string())
}

fn parse_xml(xml: &str) -> Result<XmlNode, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack = Vec::new();
    let mut root = None;

    loop {
        match reader.read_event().map_err(|error| error.to_string())? {
            Event::Start(event) => stack.push(XmlNode {
                name: event.name().as_ref().to_string(),
                attributes: attributes(&event)?,
                content: Vec::new(),
            }),
            Event::Empty(event) => append_node(
                &mut root,
                &mut stack,
                XmlNode {
                    name: event.name().as_ref().to_string(),
                    attributes: attributes(&event)?,
                    content: Vec::new(),
                },
            )?,
            Event::End(_) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| "unexpected XML closing tag".to_string())?;
                append_node(&mut root, &mut stack, node)?;
            }
            Event::Text(event) => {
                if !event.xml_content(XmlVersion::Explicit1_0).trim().is_empty() {
                    stack
                        .last_mut()
                        .ok_or_else(|| "text outside XML root".to_string())?
                        .content
                        .push(XmlContent::Text(
                            event.into_owned().into_inner().into_owned(),
                        ));
                }
            }
            Event::CData(event) => {
                if !event.xml_content(XmlVersion::Explicit1_0).trim().is_empty() {
                    stack
                        .last_mut()
                        .ok_or_else(|| "CDATA outside XML root".to_string())?
                        .content
                        .push(XmlContent::CData(event.into_owned().into_inner().into_owned()));
                }
            }
            Event::GeneralRef(event) => {
                stack
                    .last_mut()
                    .ok_or_else(|| "entity reference outside XML root".to_string())?
                    .content
                    .push(XmlContent::Reference(event.into_owned().into_inner().into_owned()));
            }
            Event::Decl(_) | Event::Comment(_) | Event::DocType(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }

    if !stack.is_empty() {
        return Err("unexpected end of XML document".to_string());
    }
    root.ok_or_else(|| "XML document has no root element".to_string())
}

fn append_node(
    root: &mut Option<XmlNode>,
    stack: &mut [XmlNode],
    node: XmlNode,
) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.content.push(XmlContent::Element(node));
    } else if root.is_none() {
        *root = Some(node);
    } else {
        return Err("XML document has multiple root elements".to_string());
    }
    Ok(())
}

fn attributes(event: &BytesStart<'_>) -> Result<Vec<(String, String)>, String> {
    event
        .attributes()
        .map(|attribute| {
            let attribute = attribute.map_err(|error| error.to_string())?;
            Ok((
                attribute.key.as_ref().to_string(),
                attribute.value.into_owned(),
            ))
        })
        .collect()
}

fn normalize_node(mut node: XmlNode) -> Result<XmlNode, String> {
    let name = local_name(&node.name);
    node.attributes.retain(|(attribute, _)| {
        !(name == "Principal" && local_name(attribute) == "id")
            && !(name == "Actions" && local_name(attribute) == "Context")
    });
    node.attributes.sort_by(|left, right| left.0.cmp(&right.0));

    let mut normalized_content = Vec::new();
    for content in node.content {
        match content {
            XmlContent::Element(child) if keep_child(&name, &child) => {
                normalized_content.push(XmlContent::Element(normalize_node(child)?));
            }
            XmlContent::Element(_) => {}
            content => normalized_content.push(content),
        }
    }
    node.content = normalized_content;
    if name == "RegistrationInfo" {
        node.content.clear();
    }
    sort_owned_children(&name, &mut node.content);
    if name == "Task" {
        sort_task_children(&mut node.content);
    }
    Ok(node)
}

fn keep_child(parent: &str, child: &XmlNode) -> bool {
    let child_name = local_name(&child.name);
    match parent {
        "RegistrationInfo" => false,
        "CalendarTrigger" => {
            !matches!(child_name.as_str(), "StartBoundary" | "Enabled" | "EndBoundary")
        }
        "LogonTrigger" | "BootTrigger" | "TimeTrigger" => {
            child_name != "Enabled"
                && !(child_name == "Delay" && text_content(child).trim() == "PT0S")
        }
        "Principal" => matches!(child_name.as_str(), "RunLevel" | "LogonType"),
        "Settings" => matches!(
            child_name.as_str(),
            "StartWhenAvailable" | "DisallowStartIfOnBatteries" | "StopIfGoingOnBatteries"
        ),
        _ => true,
    }
}

fn text_content(node: &XmlNode) -> String {
    node.content
        .iter()
        .filter_map(|content| match content {
            XmlContent::Text(text) | XmlContent::CData(text) => Some(text.as_str()),
            XmlContent::Element(_) | XmlContent::Reference(_) => None,
        })
        .collect()
}

fn sort_owned_children(parent: &str, content: &mut [XmlContent]) {
    let order = |child: &str| match (parent, child) {
        ("Principal", "RunLevel") => 0,
        ("Principal", "LogonType") => 1,
        ("Settings", "StartWhenAvailable") => 0,
        ("Settings", "DisallowStartIfOnBatteries") => 1,
        ("Settings", "StopIfGoingOnBatteries") => 2,
        _ => 3,
    };
    if !matches!(parent, "Principal" | "Settings") {
        return;
    }
    content.sort_by_key(|content| match content {
        XmlContent::Element(node) => order(&local_name(&node.name)),
        _ => 3,
    });
}

fn sort_task_children(content: &mut [XmlContent]) {
    content.sort_by_key(|content| match content {
        XmlContent::Element(node) => match local_name(&node.name).as_str() {
            "RegistrationInfo" => 0,
            "Triggers" => 1,
            "Principals" => 2,
            "Settings" => 3,
            "Actions" => 4,
            _ => 5,
        },
        _ => 5,
    });
}

fn write_normalized_node(writer: &mut XmlWriter, node: &XmlNode) -> Result<(), String> {
    let mut start = BytesStart::new(node.name.as_str());
    for (name, value) in &node.attributes {
        start.push_attribute((name.as_str(), value.as_str()));
    }
    if node.content.is_empty() {
        writer
            .write_event(Event::Empty(start))
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    writer
        .write_event(Event::Start(start))
        .map_err(|error| error.to_string())?;
    for content in &node.content {
        match content {
            XmlContent::Element(child) => write_normalized_node(writer, child)?,
            XmlContent::Text(text) => writer
                .write_event(Event::Text(BytesText::from_escaped(text.as_str())))
                .map_err(|error| error.to_string())?,
            XmlContent::CData(text) => writer
                .write_event(Event::Text(BytesText::new(text)))
                .map_err(|error| error.to_string())?,
            XmlContent::Reference(reference) => writer
                .write_event(Event::GeneralRef(quick_xml::events::BytesRef::new(
                    reference.as_str(),
                )))
                .map_err(|error| error.to_string())?,
        }
    }
    writer
        .write_event(Event::End(BytesEnd::new(node.name.as_str())))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn local_name(name: &str) -> String {
    name.rsplit_once(':')
        .map_or_else(|| name.to_string(), |(_, local)| local.to_string())
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
