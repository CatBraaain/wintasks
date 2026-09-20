mod common;

use chrono::NaiveDate;
use common::definitions;
use wintasks::def::parse_defs;
use wintasks::xml::render_task_xml;

fn render(expression: &str) -> Result<String, String> {
    let yaml = definitions(&format!(
        "  - name: task\n    trigger: {{ type: cron, value: '{expression}' }}\n    action: {{ command: cmd.exe }}\n"
    ));
    let definitions = parse_defs(&yaml, "wintasks.yaml").map_err(|error| error.to_string())?;
    let now = NaiveDate::from_ymd_opt(2026, 1, 15)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap();
    render_task_xml(&definitions.tasks[0], now).map_err(|error| error.to_string())
}

#[test]
fn parses_documented_field_forms_and_rejects_invalid_extensions() {
    for expression in [
        "*/15 9-17 * * MON-FRI",
        "0 12 1-3,5 JAN,DEC *",
        "3/6 3/6 * * *",
        "0 12 ? * ?",
    ] {
        assert!(render(expression).is_ok(), "{expression}");
    }
    for expression in ["@daily", "1,3/2 * * * *", "* * * */5 *", "* * * * 7"] {
        assert!(render(expression).is_err(), "{expression}");
    }
}

#[test]
fn decomposes_cron_schedules_and_corrects_past_boundaries() {
    let output = render("*/15 9-17 * * *").unwrap();
    assert!(output.contains("<StartBoundary>2026-01-16T09:00:00</StartBoundary>"));
    assert!(output.contains("<Interval>PT15M</Interval>"));
    assert!(output.contains("<Duration>PT9H</Duration>"));

    let output = render("0,30 9,21 * * *").unwrap();
    assert_eq!(output.matches("<CalendarTrigger>").count(), 4);

    let output = render("0 12 1 * MON").unwrap();
    assert_eq!(output.matches("<CalendarTrigger>").count(), 2);
    assert!(output.contains("<ScheduleByMonth>"));
    assert!(output.contains("<ScheduleByMonthDayOfWeek>"));
}

#[test]
fn sunday_started_weekday_ranges_are_valid() {
    for expression in ["0 12 * * 0-2", "0 12 * * SUN-TUE"] {
        let output = render(expression).unwrap();
        assert!(output.contains("<Sunday/>"), "{expression}: {output}");
        assert!(output.contains("<Tuesday/>"), "{expression}: {output}");
    }
}

#[test]
fn n_slash_one_steps_from_n_to_the_field_maximum() {
    let output = render("3/1 9 * * *").unwrap();
    assert_eq!(output.matches("<CalendarTrigger>").count(), 1);
    assert!(output.contains("<StartBoundary>2026-01-16T09:03:00</StartBoundary>"));
    assert!(output.contains("<Interval>PT1M</Interval>"));
    assert!(output.contains("<Duration>PT57M</Duration>"));
}
