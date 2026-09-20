//! Pure desired-state XML rendering.

use chrono::NaiveDateTime;

use crate::def::TaskDef;
use crate::xml::{XmlError, render_task_xml};

pub fn render_output(definitions: &[TaskDef], now: NaiveDateTime) -> Result<String, XmlError> {
    let mut output = String::new();
    for definition in definitions {
        output.push_str(&format!("--- {} ---\n", definition.name));
        output.push_str(&render_task_xml(definition, now)?);
        output.push('\n');
    }
    Ok(output)
}
