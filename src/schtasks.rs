//! schtasks invocation and best-effort query parsing.

use std::process::Command;

const UTF8_XML_DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
const UTF16_XML_DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-16"?>"#;

use quick_xml::events::Event;
use quick_xml::{Reader, Writer, XmlVersion};

#[derive(Clone)]
pub struct ExistingTask {
    pub path: String,
    pub xml: String,
}

#[derive(Debug)]
pub struct SchtasksError(pub String);

pub trait Schtasks {
    fn query(&mut self) -> Result<String, SchtasksError>;
    fn create(&mut self, path: &str, xml: &str) -> Result<(), SchtasksError>;
    fn delete(&mut self, path: &str) -> Result<(), SchtasksError>;
}

pub struct CommandSchtasks;

impl CommandSchtasks {
    fn run(args: &[&str]) -> Result<std::process::Output, SchtasksError> {
        Command::new("schtasks")
            .args(args)
            .output()
            .map_err(|error| SchtasksError(format!("failed to run schtasks: {error}")))
    }
}

impl Schtasks for CommandSchtasks {
    fn query(&mut self) -> Result<String, SchtasksError> {
        let output = Self::run(&["/Query", "/XML"])?;
        if !output.status.success() {
            return Err(SchtasksError(format!(
                "schtasks /Query /XML failed: {}",
                decode(&output.stderr)
            )));
        }
        Ok(decode(&output.stdout))
    }

    fn create(&mut self, path: &str, xml: &str) -> Result<(), SchtasksError> {
        let file = write_temp_xml(xml)?;
        let result = Self::run(&["/Create", "/XML", &file, "/TN", path, "/F"]);
        let _ = std::fs::remove_file(&file);
        match result {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => Err(SchtasksError(decode(&output.stderr))),
            Err(error) => Err(error),
        }
    }

    fn delete(&mut self, path: &str) -> Result<(), SchtasksError> {
        match Self::run(&["/Delete", "/TN", path, "/F"]) {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => Err(SchtasksError(decode(&output.stderr))),
            Err(error) => Err(error),
        }
    }
}

pub fn decode(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    } else if let Some(bytes) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_be_bytes(*chunk))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn write_temp_xml(xml: &str) -> Result<String, SchtasksError> {
    use std::io::Write;

    let path = std::env::temp_dir().join(format!("wintasks-{}.xml", std::process::id()));
    let mut file = std::fs::File::create(&path)
        .map_err(|error| SchtasksError(format!("cannot create temp XML file: {error}")))?;
    file.write_all(&encode_task_xml(xml))
        .map_err(|error| SchtasksError(format!("cannot write temp XML file: {error}")))?;
    Ok(path.to_string_lossy().into_owned())
}

// schtasks requires the XML declaration and bytes to use the same UTF-16 LE encoding.
// Keep the public XML UTF-8 so query normalization and diff output remain unchanged.
fn encode_task_xml(xml: &str) -> Vec<u8> {
    let xml = xml.strip_prefix(UTF8_XML_DECLARATION).map_or_else(
        || xml.to_owned(),
        |rest| format!("{UTF16_XML_DECLARATION}{rest}"),
    );
    let mut bytes = Vec::with_capacity(2 + xml.len() * 2);
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

/// Parses concatenated Task XML documents until the first malformed document.
pub fn extract_tasks(xml_text: &str) -> Vec<ExistingTask> {
    let mut reader = Reader::from_str(xml_text);
    reader.config_mut().trim_text(false);
    let mut depth: usize = 0;
    let mut path = String::new();
    let mut capture_uri = false;
    let mut task_xml: Option<Writer<Vec<u8>>> = None;
    let mut tasks = Vec::new();

    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(event) => {
                depth += 1;
                let name = local_name(event.name().as_ref());
                if depth == 1 && name == "Task" {
                    let mut writer = Writer::new(Vec::new());
                    writer
                        .write_event(Event::Start(event.to_owned()))
                        .expect("writing to Vec succeeds");
                    task_xml = Some(writer);
                    path.clear();
                } else if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(Event::Start(event.to_owned()))
                        .expect("writing to Vec succeeds");
                }
                capture_uri = depth == 3 && name == "URI";
            }
            Event::Empty(event) => {
                if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(Event::Empty(event.to_owned()))
                        .expect("writing to Vec succeeds");
                }
            }
            Event::Text(text) => {
                if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(Event::Text(text.to_owned()))
                        .expect("writing to Vec succeeds");
                }
                if capture_uri {
                    path.push_str(&text.xml_content(XmlVersion::Explicit1_0));
                }
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(Event::End(event.to_owned()))
                        .expect("writing to Vec succeeds");
                }
                if depth == 1 && name == "Task" {
                    if !path.is_empty() {
                        let xml = task_xml.take().expect("Task has XML").into_inner();
                        tasks.push(ExistingTask {
                            path: uri_to_task_path(&path),
                            xml: String::from_utf8(xml).expect("input XML is UTF-8"),
                        });
                    } else {
                        task_xml = None;
                    }
                }
                capture_uri = false;
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            event => {
                if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(event.into_owned())
                        .expect("writing to Vec succeeds");
                }
            }
        }
    }
    tasks
}

fn local_name(name: &str) -> String {
    name.rsplit_once(':')
        .map_or_else(|| name.to_string(), |(_, name)| name.to_string())
}

fn uri_to_task_path(uri: &str) -> String {
    uri.trim().replace('/', "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_schtasks_xml_as_utf16_le_with_matching_declaration() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Task><Command>日本語</Command></Task>"#;
        let path = write_temp_xml(xml).unwrap();
        let encoded = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(&encoded[..2], &[0xFF, 0xFE]);
        assert_eq!(decode(&encoded), xml.replace("UTF-8", "UTF-16"));
    }
}
