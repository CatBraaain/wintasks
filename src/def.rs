//! Task definition YAML: schema types, parsing, and file-level validation.

use std::fmt;

use serde::Deserialize;

pub const DESCRIPTION_MARKER: &str = "managed-by: wintasks;";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDef {
    pub name: String,
    #[serde(deserialize_with = "de_single_or_vec")]
    pub trigger: Vec<TriggerDef>,
    #[serde(deserialize_with = "de_single_or_vec")]
    pub action: Vec<ActionDef>,
    pub setting: Option<SettingDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDef {
    #[serde(rename = "type")]
    pub kind: TriggerKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Cron,
    Startup,
    Boot,
    Once,
    Now,
}

impl TriggerKind {
    /// Sub-folder name under the mount folder: `<mount>\<type>\<name>`.
    pub fn folder(self) -> &'static str {
        match self {
            TriggerKind::Cron => "cron",
            TriggerKind::Startup => "startup",
            TriggerKind::Boot => "boot",
            TriggerKind::Once => "once",
            TriggerKind::Now => "now",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDef {
    pub command: String,
    pub args: Option<String>,
    pub working_directory: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingDef {
    pub run_as: Option<BoolOrString>,
    pub logon_type: Option<LogonType>,
}

/// `run_as` accepts both a YAML boolean and the strings "true"/"false".
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum BoolOrString {
    Bool(bool),
    Str(String),
}

impl BoolOrString {
    /// None when the string is neither "true" nor "false".
    pub fn to_bool(&self) -> Option<bool> {
        match self {
            BoolOrString::Bool(b) => Some(*b),
            BoolOrString::Str(s) => match s.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogonType {
    InteractiveToken,
    S4u,
}

impl LogonType {
    pub fn xml_name(self) -> &'static str {
        match self {
            LogonType::InteractiveToken => "InteractiveToken",
            LogonType::S4u => "S4U",
        }
    }
}

/// Effective (defaults applied) settings used for XML generation and def-hash.
#[derive(Debug, Clone, Copy)]
pub struct EffectiveSetting {
    pub run_level_highest: bool,
    pub logon_type: LogonType,
}

impl TaskDef {
    pub fn effective_setting(&self) -> Result<EffectiveSetting, String> {
        match &self.setting {
            None => Ok(EffectiveSetting {
                run_level_highest: false,
                logon_type: LogonType::InteractiveToken,
            }),
            Some(s) => {
                let run_level_highest = match &s.run_as {
                    None => false,
                    Some(v) => v.to_bool().ok_or_else(|| {
                        format!("setting.run_as must be true or false, got {v:?}")
                    })?,
                };
                Ok(EffectiveSetting {
                    run_level_highest,
                    logon_type: s.logon_type.unwrap_or(LogonType::InteractiveToken),
                })
            }
        }
    }

    /// `working_directory` resolved: explicit value, or the parent directory of
    /// `command`, or None when `command` has no parent (bare file name).
    pub fn resolve_working_directory(action: &ActionDef) -> Option<String> {
        action
            .working_directory
            .clone()
            .or_else(|| parent_directory(&action.command))
    }
}

fn parent_directory(path: &str) -> Option<String> {
    // Task definitions are written for Windows, so both separators are accepted
    // the same way Path::parent handles them on Windows.
    let normalized = path.replace('/', "\\");
    match normalized.rfind('\\') {
        Some(0) => None, // "\cmd.exe" has no parent directory
        Some(i) => Some(path[..i].to_string()),
        None => None, // bare file name such as "cmd.exe"
    }
}

/// Error with the YAML source location attached when serde reports one.
#[derive(Debug)]
pub struct DefError {
    pub message: String,
}

impl fmt::Display for DefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub fn parse_defs(text: &str, file: &str) -> Result<Vec<TaskDef>, DefError> {
    // A null document (e.g. a wrong --path) must be an error, not an
    // implicit empty state: with --prune it would delete everything.
    if matches!(
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text),
        Ok(v) if v.is_null()
    ) {
        return Err(DefError {
            message: format!("{file}: no task definitions found (file is empty or null)"),
        });
    }
    let defs: Vec<TaskDef> = serde_yaml_ng::from_str(text).map_err(|e| {
        let location = e
            .location()
            .map(|l| format!("{file}:{}:{}", l.line(), l.column()));
        match location {
            Some(loc) => DefError {
                message: format!("{loc}: {e}"),
            },
            None => DefError {
                message: format!("{file}: {e}"),
            },
        }
    })?;
    check_unique_names(&defs, file)?;
    Ok(defs)
}

fn check_unique_names(defs: &[TaskDef], file: &str) -> Result<(), DefError> {
    let mut seen = std::collections::HashSet::new();
    for def in defs {
        if !seen.insert(def.name.clone()) {
            return Err(DefError {
                message: format!("{file}: duplicate task name `{}`", def.name),
            });
        }
    }
    Ok(())
}

fn de_single_or_vec<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: serde::de::DeserializeOwned,
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    // Going through Value keeps untagged-enum noise out of the message: a
    // bad trigger type surfaces as `unknown variant `daily`, expected ...`.
    let value = serde_yaml_ng::Value::deserialize(deserializer)?;
    match value {
        serde_yaml_ng::Value::Sequence(seq) => seq
            .into_iter()
            .map(|item| serde_yaml_ng::from_value::<T>(item).map_err(D::Error::custom))
            .collect(),
        single => {
            let item = serde_yaml_ng::from_value::<T>(single).map_err(D::Error::custom)?;
            Ok(vec![item])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Vec<TaskDef>, DefError> {
        parse_defs(text, "wintasks.yaml")
    }

    #[test]
    fn parses_single_trigger_and_action_into_vecs() {
        let defs = parse(
            "- name: hello\n  trigger: { type: cron, value: \"00 09 * * *\" }\n  action: { command: cmd.exe }\n",
        )
        .unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].trigger.len(), 1);
        assert_eq!(defs[0].action.len(), 1);
        assert_eq!(defs[0].trigger[0].kind, TriggerKind::Cron);
    }

    #[test]
    fn parses_trigger_and_action_arrays() {
        let defs = parse(
            "- name: hello\n  trigger:\n    - { type: cron, value: \"00 09 * * *\" }\n    - { type: startup, value: \"01:00\" }\n  action:\n    - { command: cmd.exe }\n    - { command: powershell.exe }\n",
        )
        .unwrap();
        assert_eq!(defs[0].trigger.len(), 2);
        assert_eq!(defs[0].action.len(), 2);
    }

    #[test]
    fn rejects_unknown_key_with_location() {
        let err = parse("- name: x\n  trigger: { type: cron, value: \"* * * * *\" }\n  action: { command: c }\n  extra: 1\n").unwrap_err();
        assert!(err.message.contains("wintasks.yaml:"), "{}", err.message);
        assert!(err.message.contains("extra"), "{}", err.message);
    }

    #[test]
    fn rejects_missing_required_keys() {
        let err = parse("- name: x\n  action: { command: c }\n").unwrap_err();
        assert!(err.message.contains("trigger"), "{}", err.message);
    }

    #[test]
    fn rejects_invalid_trigger_type() {
        let err =
            parse("- name: x\n  trigger: { type: daily, value: v }\n  action: { command: c }\n")
                .unwrap_err();
        assert!(err.message.contains("daily"), "{}", err.message);
    }
    #[test]
    fn rejects_duplicate_names() {
        let err = parse(
            "- name: dup\n  trigger: { type: now, value: v }\n  action: { command: c }\n- name: dup\n  trigger: { type: now, value: v }\n  action: { command: c }\n",
        )
        .unwrap_err();
        assert!(
            err.message.contains("duplicate task name `dup`"),
            "{}",
            err.message
        );
    }

    #[test]
    fn rejects_empty_document() {
        // A missing or empty file is more likely a wrong --path than an
        // intentional empty state, and prune would delete everything.
        let err = parse("").unwrap_err();
        assert!(err.message.contains("wintasks.yaml"), "{}", err.message);
    }

    #[test]
    fn rejects_null_document() {
        let err = parse("# comment only\n---\n").unwrap_err();
        assert!(err.message.contains("wintasks.yaml"), "{}", err.message);
    }

    #[test]
    fn accepts_empty_explicit_list() {
        let defs = parse("[]\n").unwrap();
        assert!(defs.is_empty());
    }

    #[test]
    fn run_as_accepts_bool_and_strings() {
        let defs = parse(
            "- name: a\n  trigger: { type: now, value: v }\n  action: { command: c }\n  setting: { run_as: true }\n- name: b\n  trigger: { type: now, value: v }\n  action: { command: c }\n  setting: { run_as: \"false\" }\n",
        )
        .unwrap();
        assert!(defs[0].effective_setting().unwrap().run_level_highest);
        assert!(!defs[1].effective_setting().unwrap().run_level_highest);
    }

    #[test]
    fn run_as_rejects_other_strings() {
        let defs = parse(
            "- name: a\n  trigger: { type: now, value: v }\n  action: { command: c }\n  setting: { run_as: \"yes\" }\n",
        )
        .unwrap();
        assert!(defs[0].effective_setting().is_err());
    }

    #[test]
    fn rejects_invalid_logon_type() {
        let err = parse("- name: a\n  trigger: { type: now, value: v }\n  action: { command: c }\n  setting: { logon_type: password }\n").unwrap_err();
        assert!(err.message.contains("logon_type"), "{}", err.message);
    }

    #[test]
    fn logon_type_s4u_maps_to_xml_name() {
        let defs = parse(
            "- name: a\n  trigger: { type: boot, value: \"00:00\" }\n  action: { command: c }\n  setting: { logon_type: s4u }\n",
        )
        .unwrap();
        assert_eq!(
            defs[0].effective_setting().unwrap().logon_type.xml_name(),
            "S4U"
        );
    }

    #[test]
    fn working_directory_defaults_to_command_parent() {
        let action = ActionDef {
            command: "C:\\Tools\\app.exe".to_string(),
            args: None,
            working_directory: None,
        };
        assert_eq!(
            TaskDef::resolve_working_directory(&action),
            Some("C:\\Tools".to_string())
        );
    }

    #[test]
    fn working_directory_none_for_bare_command() {
        let action = ActionDef {
            command: "app.exe".to_string(),
            args: None,
            working_directory: None,
        };
        assert_eq!(TaskDef::resolve_working_directory(&action), None);
    }

    #[test]
    fn working_directory_forward_slash_command() {
        let action = ActionDef {
            command: "C:/Tools/app.exe".to_string(),
            args: None,
            working_directory: None,
        };
        // The parent is derived from the original spelling.
        assert_eq!(
            TaskDef::resolve_working_directory(&action),
            Some("C:/Tools".to_string())
        );
    }

    #[test]
    fn working_directory_root_command_has_no_parent() {
        let action = ActionDef {
            command: "\\app.exe".to_string(),
            args: None,
            working_directory: None,
        };
        assert_eq!(TaskDef::resolve_working_directory(&action), None);
    }
}
