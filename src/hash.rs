//! def-hash: SHA-256 over the effective trigger/action/setting definition.
//!
//! The hash covers what lands in the XML (working directory resolved,
//! defaults applied), but not the task name or mount folder, so renaming a
//! task or moving the mount does not force a re-registration.

use sha2::{Digest, Sha256};

use crate::def::TaskDef;

pub fn def_hash(def: &TaskDef) -> Result<String, String> {
    let mut canonical = String::new();
    push_str(&mut canonical, "T");
    for trigger in &def.trigger {
        push_str(&mut canonical, trigger.kind.folder());
        push_str(&mut canonical, &trigger.value);
    }
    push_str(&mut canonical, "A");
    for action in &def.action {
        let working_directory = TaskDef::resolve_working_directory(action);
        push_str(&mut canonical, &action.command);
        push_opt(&mut canonical, &action.args);
        push_opt(&mut canonical, &working_directory);
    }
    let setting = def.effective_setting()?;
    push_str(&mut canonical, "S");
    push_str(
        &mut canonical,
        if setting.run_level_highest { "1" } else { "0" },
    );
    push_str(&mut canonical, setting.logon_type.xml_name());

    let digest = Sha256::digest(canonical.as_bytes());
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Length-prefixed so values containing separators cannot collide.
fn push_str(out: &mut String, value: &str) {
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
}

fn push_opt(out: &mut String, value: &Option<String>) {
    match value {
        Some(v) => push_str(out, v),
        None => out.push_str("0:"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::{ActionDef, LogonType, SettingDef, TriggerDef, TriggerKind};

    fn sample_def(name: &str) -> TaskDef {
        TaskDef {
            name: name.to_string(),
            trigger: vec![TriggerDef {
                kind: TriggerKind::Cron,
                value: "00 09 * * *".to_string(),
            }],
            action: vec![ActionDef {
                command: "cmd.exe".to_string(),
                args: Some("/c echo hi".to_string()),
                working_directory: None,
            }],
            setting: None,
        }
    }

    #[test]
    fn hash_is_64_hex_chars_and_stable() {
        let a = def_hash(&sample_def("task")).unwrap();
        let b = def_hash(&sample_def("task")).unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, b);
    }

    #[test]
    fn hash_ignores_name() {
        assert_eq!(
            def_hash(&sample_def("one")).unwrap(),
            def_hash(&sample_def("two")).unwrap()
        );
    }

    #[test]
    fn hash_changes_when_definition_changes() {
        let mut def = sample_def("task");
        def.action[0].args = Some("/c echo bye".to_string());
        assert_ne!(
            def_hash(&sample_def("task")).unwrap(),
            def_hash(&def).unwrap()
        );
    }

    #[test]
    fn hash_changes_when_setting_changes() {
        let mut def = sample_def("task");
        def.setting = Some(SettingDef {
            run_as: Some(crate::def::BoolOrString::Bool(true)),
            logon_type: Some(LogonType::S4u),
        });
        assert_ne!(
            def_hash(&sample_def("task")).unwrap(),
            def_hash(&def).unwrap()
        );
    }

    #[test]
    fn hash_same_for_explicit_working_directory_matching_default() {
        let mut implicit = sample_def("task");
        implicit.action[0].command = "C:\\Tools\\app.exe".to_string();
        implicit.action[0].working_directory = None; // resolves to C:\Tools
        let mut explicit = implicit.clone();
        explicit.action[0].working_directory = Some("C:\\Tools".to_string());
        assert_eq!(def_hash(&implicit).unwrap(), def_hash(&explicit).unwrap());
    }

    #[test]
    fn hash_changes_when_cron_value_text_changes() {
        let mut def = sample_def("task");
        def.trigger[0].value = "0 9 * * *".to_string();
        assert_ne!(
            def_hash(&sample_def("task")).unwrap(),
            def_hash(&def).unwrap()
        );
    }
}
