//! YAML schema and validation for the declarative desired state.

use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definitions {
    pub mount: String,
    pub tasks: Vec<TaskDef>,
}

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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum BoolOrString {
    Bool(bool),
    Str(String),
}

impl BoolOrString {
    fn to_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Str(value) => match value.as_str() {
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
            Self::InteractiveToken => "InteractiveToken",
            Self::S4u => "S4U",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EffectiveSetting {
    pub run_level_highest: bool,
    pub logon_type: LogonType,
}

impl TaskDef {
    pub fn effective_setting(&self) -> Result<EffectiveSetting, String> {
        let Some(setting) = &self.setting else {
            return Ok(EffectiveSetting {
                run_level_highest: false,
                logon_type: LogonType::InteractiveToken,
            });
        };
        let run_level_highest = setting
            .run_as
            .as_ref()
            .map(BoolOrString::to_bool)
            .unwrap_or(Some(false))
            .ok_or_else(|| "setting.run_as must be true or false".to_string())?;
        Ok(EffectiveSetting {
            run_level_highest,
            logon_type: setting.logon_type.unwrap_or(LogonType::InteractiveToken),
        })
    }

    pub fn resolve_working_directory(action: &ActionDef) -> Option<String> {
        action
            .working_directory
            .clone()
            .or_else(|| parent_directory(&action.command))
    }
}

fn parent_directory(path: &str) -> Option<String> {
    let normalized = path.replace('/', "\\");
    match normalized.rfind('\\') {
        Some(0) | None => None,
        Some(index) => Some(path[..index].to_string()),
    }
}

#[derive(Debug)]
pub struct DefError {
    pub message: String,
}

impl fmt::Display for DefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub fn parse_defs(text: &str, file: &str, mount: &str) -> Result<Definitions, DefError> {
    if matches!(
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text),
        Ok(value) if value.is_null()
    ) {
        return Err(DefError {
            message: format!("{file}: no task definitions found (file is empty or null)"),
        });
    }
    let tasks = serde_yaml_ng::from_str(text).map_err(|error| yaml_error(file, error))?;
    let definitions = Definitions {
        mount: mount.to_string(),
        tasks,
    };
    validate(&definitions, file)?;
    Ok(definitions)
}

fn yaml_error(file: &str, error: serde_yaml_ng::Error) -> DefError {
    let prefix = error.location().map_or_else(
        || file.to_string(),
        |location| format!("{file}:{}:{}", location.line(), location.column()),
    );
    DefError {
        message: format!("{prefix}: {error}"),
    }
}

fn validate(definitions: &Definitions, file: &str) -> Result<(), DefError> {
    if !valid_mount(&definitions.mount) {
        return Err(DefError {
            message: format!("{file}: mount must be a non-root folder path"),
        });
    }

    let mut names = HashSet::new();
    for task in &definitions.tasks {
        if !names.insert(&task.name) {
            return Err(DefError {
                message: format!("{file}: duplicate task name `{}`", task.name),
            });
        }
        if task.trigger.is_empty() {
            return Err(DefError {
                message: format!("{file}: task `{}` must define a trigger", task.name),
            });
        }
        if task.action.is_empty() {
            return Err(DefError {
                message: format!("{file}: task `{}` must define an action", task.name),
            });
        }
    }
    Ok(())
}

fn valid_mount(mount: &str) -> bool {
    !mount.is_empty()
        && !mount.starts_with('\\')
        && !mount.ends_with('\\')
        && mount.split('\\').all(|part| !part.is_empty())
}

fn de_single_or_vec<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: serde::de::DeserializeOwned,
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    match serde_yaml_ng::Value::deserialize(deserializer)? {
        serde_yaml_ng::Value::Sequence(items) => items
            .into_iter()
            .map(|item| serde_yaml_ng::from_value(item).map_err(D::Error::custom))
            .collect(),
        item => serde_yaml_ng::from_value(item)
            .map(|item| vec![item])
            .map_err(D::Error::custom),
    }
}
