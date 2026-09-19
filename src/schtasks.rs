//! schtasks invocation, isolated behind a trait so tests never need Windows.

use std::collections::BTreeMap;
use std::process::Command;

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use crate::def::DESCRIPTION_MARKER;

pub struct ManagedTask {
    /// Task Scheduler path such as `\WinTasks\cron\name`.
    pub path: String,
    /// def-hash parsed from the Description marker; None when unreadable,
    /// which forces an update comparison mismatch.
    pub def_hash: Option<String>,
}

#[derive(Debug)]
pub struct SchtasksError(pub String);

pub trait Schtasks {
    /// All task definitions as XML, via `schtasks /Query /XML`.
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
            .map_err(|e| SchtasksError(format!("failed to run schtasks: {e}")))
    }
}

impl Schtasks for CommandSchtasks {
    fn query(&mut self) -> Result<String, SchtasksError> {
        let out = CommandSchtasks::run(&["/Query", "/XML"])?;
        if !out.status.success() {
            return Err(schtasks_query_error(&out));
        }
        Ok(decode(&out.stdout))
    }

    fn create(&mut self, path: &str, xml: &str) -> Result<(), SchtasksError> {
        let file = write_temp_xml(xml)?;
        let result = CommandSchtasks::run(&["/Create", "/TN", path, "/XML", &file, "/F"]);
        let _ = std::fs::remove_file(&file);
        match result {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(SchtasksError(format!(
                "schtasks /Create /TN {path} failed: {}",
                decode(&out.stderr)
            ))),
            Err(e) => Err(e),
        }
    }

    fn delete(&mut self, path: &str) -> Result<(), SchtasksError> {
        match CommandSchtasks::run(&["/Delete", "/TN", path, "/F"]) {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(SchtasksError(format!(
                "schtasks /Delete /TN {path} failed: {}",
                decode(&out.stderr)
            ))),
            Err(e) => Err(e),
        }
    }
}

fn schtasks_query_error(out: &std::process::Output) -> SchtasksError {
    SchtasksError(format!(
        "schtasks /Query /XML failed: {}",
        decode(&out.stderr)
    ))
}

/// schtasks XML output is UTF-16 when it carries a BOM, otherwise OEM/ANSI
/// bytes that we can only decode lossily.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// schtasks /Create /XML takes a file, not stdin: park the XML next to the
/// binary's temp dir and remove it after the call.
fn write_temp_xml(xml: &str) -> Result<String, SchtasksError> {
    use std::io::Write;
    let unique = std::process::id();
    let file = std::env::temp_dir().join(format!("wintasks-{unique}.xml"));
    let mut f = std::fs::File::create(&file)
        .map_err(|e| SchtasksError(format!("cannot create temp XML file: {e}")))?;
    // UTF-8 with a matching declaration attribute (spec); Windows-side
    // acceptance is a verification item.
    f.write_all(xml.as_bytes())
        .map_err(|e| SchtasksError(format!("cannot write temp XML file: {e}")))?;
    Ok(file.to_string_lossy().into_owned())
}

/// Extracts every task path plus the managed subset (Description starts
/// with the marker) from concatenated `/Query /XML` documents.
pub struct QuerySummary {
    /// All task paths found, managed or not.
    pub all_paths: std::collections::BTreeSet<String>,
    /// Tasks carrying the managed marker.
    pub managed: Vec<ManagedTask>,
}

pub fn extract_tasks(xml_text: &str) -> QuerySummary {
    let mut reader = Reader::from_str(xml_text);
    // Concatenated documents are not well-formed as a whole.
    reader.config_mut().check_end_names = false;
    reader.config_mut().trim_text(true);

    let mut tasks = Vec::new();
    let mut all_paths = std::collections::BTreeSet::new();
    let mut depth: usize = 0;
    let mut in_registration_info = false;
    let mut description = String::new();
    let mut uri = String::new();
    let mut capture: Option<Capture> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                let name: Vec<u8> = e.name().local_name().into_inner().as_bytes().to_vec();
                match (depth, name.as_slice()) {
                    (2, b"RegistrationInfo") => in_registration_info = true,
                    (3, b"Description") if in_registration_info => {
                        capture = Some(Capture::Description)
                    }
                    (3, b"URI") if in_registration_info => capture = Some(Capture::Uri),
                    _ => {}
                }
            }
            Ok(Event::Empty(_)) => {}
            Ok(Event::Text(t)) => {
                let text = t.xml_content(XmlVersion::Explicit1_0);
                match capture {
                    Some(Capture::Description) => description.push_str(&text),
                    Some(Capture::Uri) => uri.push_str(&text),
                    None => {}
                }
            }
            Ok(Event::End(e)) => {
                let name: Vec<u8> = e.name().local_name().into_inner().as_bytes().to_vec();
                match (depth, name.as_slice()) {
                    (3, b"Description") | (3, b"URI") => capture = None,
                    (2, b"RegistrationInfo") => in_registration_info = false,
                    (1, b"Task") => {
                        if !uri.is_empty() {
                            all_paths.insert(uri_to_task_path(&uri));
                        }
                        if description.starts_with(DESCRIPTION_MARKER) && !uri.is_empty() {
                            tasks.push(ManagedTask {
                                path: uri_to_task_path(&uri),
                                def_hash: parse_def_hash(&description),
                            });
                        }
                        description.clear();
                        uri.clear();
                    }
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break, // best effort: keep tasks parsed so far
        }
    }
    QuerySummary {
        all_paths,
        managed: tasks,
    }
}

enum Capture {
    Description,
    Uri,
}

/// `/WinTasks/cron/name` -> `\WinTasks\cron\name` (schtasks /TN spelling).
fn uri_to_task_path(uri: &str) -> String {
    uri.trim().replace('/', "\\")
}

fn parse_def_hash(description: &str) -> Option<String> {
    let rest = description.strip_prefix(DESCRIPTION_MARKER)?.trim_start();
    let hash = rest.strip_prefix("def-hash:")?.trim();
    let is_hex = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    if is_hex {
        Some(hash.to_ascii_lowercase())
    } else {
        None
    }
}

/// In-memory Schtasks for tests.
pub struct FakeSchtasks {
    /// path -> def hash (None mimics an unreadable hash).
    pub tasks: BTreeMap<String, Option<String>>,
    pub fail_create_paths: Vec<String>,
    pub fail_delete_paths: Vec<String>,
    pub create_calls: Vec<String>,
    pub delete_calls: Vec<String>,
}

impl FakeSchtasks {
    pub fn new() -> Self {
        FakeSchtasks {
            tasks: BTreeMap::new(),
            fail_create_paths: Vec::new(),
            fail_delete_paths: Vec::new(),
            create_calls: Vec::new(),
            delete_calls: Vec::new(),
        }
    }

    pub fn query_xml(&self) -> String {
        let mut out = String::new();
        for (path, hash) in &self.tasks {
            let uri = path.replace('\\', "/");
            let description = format!(
                "{DESCRIPTION_MARKER} def-hash: {}",
                hash.as_deref().unwrap_or("unreadable")
            );
            out.push_str(&format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Task xmlns=\"{NS}\" version=\"1.2\">\n  <RegistrationInfo>\n    <Description>{description}</Description>\n    <URI>{uri}</URI>\n  </RegistrationInfo>\n</Task>\n",
                NS = crate::xml::TASK_XML_NS
            ));
        }
        out
    }
}

impl Default for FakeSchtasks {
    fn default() -> Self {
        Self::new()
    }
}

impl Schtasks for FakeSchtasks {
    fn query(&mut self) -> Result<String, SchtasksError> {
        Ok(self.query_xml())
    }

    fn create(&mut self, path: &str, _xml: &str) -> Result<(), SchtasksError> {
        if self.fail_create_paths.iter().any(|p| p == path) {
            return Err(SchtasksError(format!(
                "schtasks /Create /TN {path} failed: ERROR ACCESS DENIED"
            )));
        }
        self.create_calls.push(path.to_string());
        Ok(())
    }

    fn delete(&mut self, path: &str) -> Result<(), SchtasksError> {
        if self.fail_delete_paths.iter().any(|p| p == path) {
            return Err(SchtasksError(format!(
                "schtasks /Delete /TN {path} failed: ERROR ACCESS DENIED"
            )));
        }
        self.delete_calls.push(path.to_string());
        Ok(())
    }
}

/// Test helper: a task document without the managed marker.
#[cfg(test)]
pub(crate) fn marker_free_task_xml(path: &str) -> String {
    let uri = path.replace('\\', "/");
    format!(
        "<?xml version=\"1.0\"?><Task xmlns=\"{NS}\"><RegistrationInfo><Description>unrelated</Description><URI>{uri}</URI></RegistrationInfo></Task>",
        NS = crate::xml::TASK_XML_NS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_xml(path: &str, description: &str) -> String {
        let uri = path.replace('\\', "/");
        format!(
            "<?xml version=\"1.0\"?><Task xmlns=\"{NS}\"><RegistrationInfo><Description>{description}</Description><URI>{uri}</URI></RegistrationInfo></Task>",
            NS = crate::xml::TASK_XML_NS
        )
    }

    #[test]
    fn extracts_managed_tasks_from_concatenated_documents() {
        let hash = "0".repeat(64);
        let xml = format!(
            "{}{}",
            task_xml(
                "\\WinTasks\\cron\\a",
                &format!("{DESCRIPTION_MARKER} def-hash: {hash}")
            ),
            task_xml("\\Other\\unmanaged", "something else")
        );
        let summary = extract_tasks(&xml);
        assert_eq!(summary.managed.len(), 1);
        assert_eq!(summary.managed[0].path, "\\WinTasks\\cron\\a");
        assert_eq!(
            summary.managed[0].def_hash.as_deref(),
            Some(&"0".repeat(64)[..])
        );
        // Both paths are known, so apply can detect unmanaged collisions.
        assert!(summary.all_paths.contains("\\WinTasks\\cron\\a"));
        assert!(summary.all_paths.contains("\\Other\\unmanaged"));
    }

    #[test]
    fn unmanaged_task_without_marker_is_ignored() {
        let xml = task_xml("\\Other\\task", "something else");
        let summary = extract_tasks(&xml);
        assert!(summary.managed.is_empty());
        assert_eq!(summary.all_paths.len(), 1);
    }

    #[test]
    fn unreadable_hash_becomes_none() {
        let xml = task_xml("\\W\\t", "managed-by: wintasks; def-hash: broken");
        let managed = extract_tasks(&xml).managed;
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].def_hash, None);
    }

    #[test]
    fn uri_slashes_convert_to_backslashes() {
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let xml = task_xml(
            "\\WinTasks\\startup\\Boot Task",
            &format!("{DESCRIPTION_MARKER} def-hash: {hash}"),
        );
        let managed = extract_tasks(&xml).managed;
        assert_eq!(managed[0].path, "\\WinTasks\\startup\\Boot Task");
    }

    #[test]
    fn decodes_utf16_with_bom() {
        let text = "abc";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode(&bytes), "abc");
    }

    #[test]
    fn decodes_utf8_lossy() {
        assert_eq!(decode("plain".as_bytes()), "plain");
    }
}
