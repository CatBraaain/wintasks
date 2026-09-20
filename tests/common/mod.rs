//! Shared integration-test helpers.

#[allow(dead_code)]
pub const FIXED_NOW: chrono::NaiveDateTime = match chrono::NaiveDate::from_ymd_opt(2026, 1, 15) {
    Some(date) => match date.and_hms_opt(10, 0, 0) {
        Some(datetime) => datetime,
        None => panic!("invalid fixed time"),
    },
    None => panic!("invalid fixed date"),
};

pub fn definitions(tasks: &str) -> String {
    tasks
        .lines()
        .map(|line| line.strip_prefix("  ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}
