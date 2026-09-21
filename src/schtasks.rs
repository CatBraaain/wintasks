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
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        text.to_owned()
    } else {
        decode_windows_code_page(bytes)
    }
}

#[cfg(windows)]
fn decode_windows_code_page(bytes: &[u8]) -> String {
    let code_page = unsafe {
        let console_code_page = GetConsoleOutputCP();
        if console_code_page == 0 {
            GetOEMCP()
        } else {
            console_code_page
        }
    };
    decode_with_code_page(bytes, code_page)
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(windows)]
fn decode_with_code_page(bytes: &[u8], code_page: u32) -> Option<String> {
    let byte_count = i32::try_from(bytes.len()).ok()?;
    unsafe {
        let required = MultiByteToWideChar(
            code_page,
            0,
            bytes.as_ptr().cast(),
            byte_count,
            std::ptr::null_mut(),
            0,
        );
        if required <= 0 {
            return None;
        }
        let mut units = vec![0u16; required as usize];
        let written = MultiByteToWideChar(
            code_page,
            0,
            bytes.as_ptr().cast(),
            byte_count,
            units.as_mut_ptr(),
            required,
        );
        (written > 0).then(|| String::from_utf16_lossy(&units[..written as usize]))
    }
}

#[cfg(not(windows))]
fn decode_windows_code_page(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetConsoleOutputCP() -> u32;
    fn GetOEMCP() -> u32;
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        multi_byte_str: *const i8,
        cb_multi_byte: i32,
        wide_char_str: *mut u16,
        cch_wide_char: i32,
    ) -> i32;
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

/// Parses Task XML documents until the first malformed document.
pub fn extract_tasks(xml_text: &str) -> Vec<ExistingTask> {
    let mut reader = Reader::from_str(xml_text);
    reader.config_mut().trim_text(false);
    let mut depth: usize = 0;
    let mut task_depth = None;
    let mut registration_info_depth = None;
    let mut path = String::new();
    let mut capture_uri = false;
    let mut task_xml: Option<Writer<Vec<u8>>> = None;
    let mut tasks = Vec::new();

    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(event) => {
                depth += 1;
                let name = local_name(event.name().as_ref());
                if task_depth.is_none() && name == "Task" {
                    let mut writer = Writer::new(Vec::new());
                    writer
                        .write_event(Event::Start(event.to_owned()))
                        .expect("writing to Vec succeeds");
                    task_xml = Some(writer);
                    task_depth = Some(depth);
                    path.clear();
                } else if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(Event::Start(event.to_owned()))
                        .expect("writing to Vec succeeds");
                }
                if task_depth == Some(depth - 1) && name == "RegistrationInfo" {
                    registration_info_depth = Some(depth);
                }
                capture_uri = registration_info_depth == Some(depth - 1) && name == "URI";
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
            Event::CData(text) => {
                if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(Event::CData(text.to_owned()))
                        .expect("writing to Vec succeeds");
                }
                if capture_uri {
                    path.push_str(text.as_ref());
                }
            }
            Event::End(event) => {
                let name = local_name(event.name().as_ref());
                if let Some(writer) = task_xml.as_mut() {
                    writer
                        .write_event(Event::End(event.to_owned()))
                        .expect("writing to Vec succeeds");
                }
                if task_depth == Some(depth) && name == "Task" {
                    if !path.is_empty() {
                        let xml = task_xml.take().expect("Task has XML").into_inner();
                        tasks.push(ExistingTask {
                            path: uri_to_task_path(&path),
                            xml: String::from_utf8(xml).expect("input XML is UTF-8"),
                        });
                    } else {
                        task_xml = None;
                    }
                    task_depth = None;
                    registration_info_depth = None;
                } else if registration_info_depth == Some(depth) && name == "RegistrationInfo" {
                    registration_info_depth = None;
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

    #[cfg(windows)]
    #[test]
    fn decodes_non_utf8_bytes_with_the_selected_windows_code_page() {
        assert_eq!(decode_with_code_page(&[0xE9], 1252).as_deref(), Some("é"));
    }
}
