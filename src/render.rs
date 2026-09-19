//! `wintasks render`: pure YAML-to-XML conversion to stdout.

use chrono::NaiveDateTime;

use crate::def::TaskDef;
use crate::xml::{XmlError, render_task_xml};

/// One `--- <name> ---` separator line per task, always printed, followed by
/// the task's XML document.
pub fn render_output(defs: &[TaskDef], now: NaiveDateTime) -> Result<String, XmlError> {
    let mut out = String::new();
    for def in defs {
        let task = render_task_xml(def, now)?;
        out.push_str(&format!("--- {} ---\n", def.name));
        out.push_str(&task.xml);
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::parse_defs;

    fn now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
    }

    #[test]
    fn separator_is_always_printed_and_xml_follows() {
        let defs = parse_defs(
            "- name: Solo\n  trigger: { type: now, value: v }\n  action: { command: c }\n",
            "wintasks.yaml",
        )
        .unwrap();
        let out = render_output(&defs, now()).unwrap();
        assert!(out.starts_with("--- Solo ---\n"), "{out}");
        assert!(
            out.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "{out}"
        );
        assert!(out.ends_with("</Task>\n"), "{out}");
    }

    #[test]
    fn multiple_tasks_each_get_a_separator() {
        let defs = parse_defs(
            "- name: A\n  trigger: { type: now, value: v }\n  action: { command: c }\n- name: B\n  trigger: { type: now, value: v }\n  action: { command: c }\n",
            "wintasks.yaml",
        )
        .unwrap();
        let out = render_output(&defs, now()).unwrap();
        assert_eq!(out.matches("--- A ---").count(), 1);
        assert_eq!(out.matches("--- B ---").count(), 1);
        assert_eq!(out.matches("<?xml").count(), 2);
    }
}
