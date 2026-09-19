//! Integration tests for cron expressions: syntax, trigger
//! decomposition, and StartBoundary handling.
//!
//! Oracle: SPEC.md — the "cron 式の構文" section (field forms,
//! per-field constraint table, normalization bullets), the "cron 式の
//! trigger 分解" section (schedule-selection table, list/range
//! bullets, time-of-day table, examples), and the "StartBoundary の
//! 決定と過去補正" section. One `#[test]` per spec table row /
//! bullet / example.

mod common;

use chrono::NaiveDateTime;
use common::{run_render, write_temp_yaml};

// ---------------------------------------------------------------- helpers

/// A one-task desired state whose cron value is `value`.
fn cron_yaml(value: &str) -> String {
    format!(
        "- name: T\n  trigger: {{ type: cron, value: \"{value}\" }}\n  action: {{ command: cmd.exe }}\n"
    )
}

/// Renders a cron expression through the CLI (exit code, stdout,
/// stderr) — the observable accept/reject surface.
fn render_cron(value: &str) -> (u8, String, String) {
    let path = write_temp_yaml("cron", &cron_yaml(value));
    run_render(&path)
}

/// Renders a cron expression through `render_output` with a pinned
/// clock, for date-dependent expectations.
fn render_cron_at(value: &str, now: NaiveDateTime) -> String {
    let defs = wintasks::def::parse_defs(&cron_yaml(value), "wintasks.yaml").expect("parse defs");
    wintasks::render::render_output(&defs, now).expect("render output")
}

/// A fixed local time.
fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.and_hms_opt(hour, minute, 0))
        .expect("valid datetime")
}

/// The `<Triggers>...</Triggers>` block of the first task.
fn triggers_of(output: &str) -> &str {
    let start = output.find("<Triggers>").expect("Triggers");
    let end = output.find("</Triggers>").expect("</Triggers>");
    &output[start..end + "</Triggers>".len()]
}

/// All StartBoundary values inside a `<Triggers>` block, in order.
fn boundaries(triggers: &str) -> Vec<&str> {
    triggers
        .split("<StartBoundary>")
        .skip(1)
        .map(|rest| rest.split("</StartBoundary>").next().unwrap())
        .collect()
}

/// The time-of-day parts (`HH:MM:SS`) of all StartBoundary values,
/// independent of the run date.
fn boundary_times(output: &str) -> Vec<String> {
    boundaries(triggers_of(output))
        .into_iter()
        .map(|b| b.split_once('T').expect("date T time").1.to_string())
        .collect()
}

fn calendar_count(output: &str) -> usize {
    output.matches("<CalendarTrigger>").count()
}

/// Collapses indentation and newlines so nested elements can be
/// compared as concatenated strings (the exact indent layout is
/// covered by the XML-format tests in render_xml.rs).
fn flat(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("")
}

/// Asserts the expression renders successfully.
fn accepted(value: &str) {
    let (code, out, err) = render_cron(value);
    assert_eq!(code, 0, "{value}: {err}");
    assert!(out.contains("<CalendarTrigger>"), "{value}: {out}");
}

/// Asserts the expression is rejected with exit 1.
fn rejected(value: &str) {
    let (code, out, err) = render_cron(value);
    assert_eq!(code, 1, "{value}: code {code}, err {err}");
    assert!(out.is_empty(), "{value}: {out}");
}

const MIDNIGHT: fn() -> NaiveDateTime = || at(2026, 1, 15, 0, 0);

// ------------------------------------------------------------------ syntax
// Spec: five whitespace-separated fields.

#[test]
fn exactly_five_fields_required() {
    accepted("* * * * *");
    rejected("* * * *");
    rejected("* * * * * *");
}

// Spec field forms: `*`, single value, range, the three step forms,
// comma lists.

#[test]
fn star_field_accepted() {
    accepted("* * * * *");
}

#[test]
fn single_values_accepted() {
    accepted("5 9 1 3 2");
}

#[test]
fn ranges_accepted_including_name_ranges() {
    // Spec: ranges may use month and weekday names as endpoints.
    accepted("5-10 9-11 1-2 JAN-FEB MON-FRI");
}

#[test]
fn step_forms_accepted() {
    // `*/n`, `n-m/n`, and `n/n` (n to the field maximum, every n).
    accepted("*/15 * * * *");
    accepted("0 0-23/6 * * *");
    accepted("10/20 * * * *");
}

#[test]
fn comma_lists_accepted() {
    // Spec examples: `1-3,5` and `MON,WED,FRI`.
    accepted("1-3,5 9 * * *");
    accepted("0 12 * * MON,WED,FRI");
}

// Spec constraint table: per-field value ranges, name notation, and
// the notes columns.

#[test]
fn minute_field_is_0_to_59() {
    accepted("0 * * * *");
    accepted("59 * * * *");
    rejected("60 * * * *");
}

#[test]
fn hour_field_is_0_to_23() {
    accepted("* 0 * * *");
    accepted("* 23 * * *");
    rejected("* 24 * * *");
}

#[test]
fn day_field_is_1_to_31() {
    accepted("* * 1 * *");
    accepted("* * 31 * *");
    rejected("* * 0 * *");
    rejected("* * 32 * *");
}

#[test]
fn month_field_is_1_to_12_with_names_and_no_steps() {
    accepted("* * * 1 *");
    accepted("* * * 12 *");
    accepted("* * * JAN *");
    rejected("* * * 13 *");
    // Spec: month steps (`*/5` etc.) are an error.
    rejected("* * * */5 *");
    rejected("* * * 1-12/2 *");
}

#[test]
fn weekday_field_is_0_to_6_with_names_mixed_with_numbers() {
    accepted("* * * * 0");
    accepted("* * * * 6");
    rejected("* * * * 7");
    // Spec: weekday steps (`MON/3` etc.) are an error.
    rejected("* * * * */2");
    rejected("* * * * MON/3");
    // Spec: names and numbers can be mixed.
    let (code, out, err) = render_cron("0 12 * * MON,3");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("<Monday/>"), "{out}");
    assert!(out.contains("<Wednesday/>"), "{out}");
}

// Spec: names are case-insensitive.

#[test]
fn names_are_case_insensitive() {
    let (code, out, err) = render_cron("0 12 * jan mon");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("<January/>"), "{out}");
    assert!(out.contains("<Monday/>"), "{out}");
}

// Spec: `?` is accepted only as the whole day or weekday field and is
// equivalent to `*`.

#[test]
fn question_mark_is_whole_field_in_day_or_weekday_only() {
    rejected("? * * * *");
    rejected("* * ?,5 * *");
    // Day and weekday `?` are accepted and behave like `*`.
    for expr in ["0 9 ? * *", "0 9 * * ?"] {
        let (code, out, err) = render_cron(expr);
        assert_eq!(code, 0, "{expr}: {err}");
        assert!(out.contains("<DaysInterval>1</DaysInterval>"), "{expr}: {out}");
    }
}

// Spec: a range covering the whole field equals `*` (steps included),
// and `n-n` equals the single value `n`.

#[test]
fn full_range_field_is_equivalent_to_star() {
    let now = MIDNIGHT();
    let star = render_cron_at("0 0 * * *", now);
    let full_range = render_cron_at("0 0 1-31 1-12 0-6", now);
    assert_eq!(triggers_of(&full_range), triggers_of(&star));

    let star_step = render_cron_at("*/15 9-17 * * *", now);
    let range_step = render_cron_at("0-59/15 9-17 * * *", now);
    assert_eq!(triggers_of(&range_step), triggers_of(&star_step));
}

#[test]
fn degenerate_range_equals_single_value() {
    let now = MIDNIGHT();
    let single = render_cron_at("5 9 * * *", now);
    let degenerate = render_cron_at("5-5 9 * * *", now);
    assert_eq!(triggers_of(&degenerate), triggers_of(&single));
}

// Spec: Quartz extensions (`L`, `W`, `#`, `@daily` macros, mixing
// lists and steps like `1,3/2`) are errors.

#[test]
fn quartz_extensions_and_mixed_step_lists_rejected() {
    rejected("L * * * *");
    rejected("15W * * * *");
    rejected("* * * * MON#2");
    rejected("@daily");
    rejected("1,3/2 * * * *");
}

// ------------------------------------------------- schedule selection rows
// Spec schedule-selection table: one row, one test.

#[test]
fn star_dow_star_day_star_month_is_schedule_by_day() {
    // Row 1: ScheduleByDay, DaysInterval = day increment.
    let (code, out, err) = render_cron("00 09 * * *");
    assert_eq!(code, 0, "{err}");
    assert_eq!(calendar_count(&out), 1, "{out}");
    assert!(out.contains("<DaysInterval>1</DaysInterval>"), "{out}");

    // A `*/n` day field uses DaysInterval = n.
    let (_, out, _) = render_cron("0 12 */2 * *");
    assert!(out.contains("<DaysInterval>2</DaysInterval>"), "{out}");
}

#[test]
fn star_dow_star_day_limited_month_is_schedule_by_month_all_days() {
    // Row 2: Day lists all values 1-31, Months the given month.
    let (code, out, err) = render_cron("0 12 * JAN *");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("<ScheduleByMonth>"), "{out}");
    assert_eq!(out.matches("<Day>").count(), 31, "{out}");
    assert!(out.contains("<Day>1</Day>"), "{out}");
    assert!(out.contains("<Day>31</Day>"), "{out}");
    assert!(flat(&out).contains("<Months><January/></Months>"), "{out}");
}

#[test]
fn star_dow_limited_day_any_month_is_schedule_by_month_day_list() {
    // Row 3: Day is the list of day values, any month.
    let (code, out, err) = render_cron("0 12 1,15 * *");
    assert_eq!(code, 0, "{err}");
    assert!(flat(&out).contains("<DaysOfMonth><Day>1</Day><Day>15</Day></DaysOfMonth>"), "{out}");
    // Month still applies when limited.
    let (_, out, _) = render_cron("0 12 1,15 JAN *");
    assert!(flat(&out).contains("<Months><January/></Months>"), "{out}");
}

#[test]
fn limited_dow_star_day_star_month_is_schedule_by_week() {
    // Row 4: ScheduleByWeek with the weekday list.
    let (code, out, err) = render_cron("0 12 * * MON,WED");
    assert_eq!(code, 0, "{err}");
    assert_eq!(calendar_count(&out), 1, "{out}");
    assert!(out.contains("<ScheduleByWeek>"), "{out}");
    assert!(flat(&out).contains("<DaysOfWeek><Monday/><Wednesday/></DaysOfWeek>"), "{out}");
}

#[test]
fn limited_dow_star_day_limited_month_is_schedule_by_month_dow() {
    // Row 5: ScheduleByMonthDayOfWeek with weekday list and months.
    let (code, out, err) = render_cron("0 12 * JAN MON");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("<ScheduleByMonthDayOfWeek>"), "{out}");
    assert!(flat(&out).contains("<DaysOfWeek><Monday/></DaysOfWeek>"), "{out}");
    assert!(flat(&out).contains("<Months><January/></Months>"), "{out}");
}

#[test]
fn limited_dow_and_day_produce_two_triggers() {
    // Row 6: ScheduleByMonth (day list) and ScheduleByMonthDayOfWeek
    // (weekday list), two independent triggers (OR semantics).
    let (code, out, err) = render_cron("0 12 1 * MON");
    assert_eq!(code, 0, "{err}");
    assert_eq!(calendar_count(&out), 2, "{out}");
    assert_eq!(out.matches("<ScheduleByMonth>").count(), 1, "{out}");
    assert_eq!(out.matches("<ScheduleByMonthDayOfWeek>").count(), 1, "{out}");
}

// Spec: a `*` month writes all twelve months; ScheduleByWeek and
// ScheduleByMonthDayOfWeek never write a Weeks element; lists are
// sorted and deduplicated; ranges expand to all values.

#[test]
fn star_month_lists_all_twelve_months() {
    let (code, out, err) = render_cron("0 12 1 * *");
    assert_eq!(code, 0, "{err}");
    let months = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ];
    for month in months {
        assert!(out.contains(&format!("<{month}/>")), "{month}: {out}");
    }
}

#[test]
fn weeks_element_is_never_written() {
    let (_, week, _) = render_cron("0 12 * * MON");
    assert!(!week.contains("<Weeks>"), "{week}");
    let (_, month_dow, _) = render_cron("0 12 * JAN MON");
    assert!(!month_dow.contains("<Weeks>"), "{month_dow}");
}

#[test]
fn lists_are_sorted_and_deduplicated() {
    // Spec: `3,1,3` -> `1,3`.
    let (code, out, err) = render_cron("0 12 3,1,3 * *");
    assert_eq!(code, 0, "{err}");
    assert!(flat(&out).contains("<DaysOfMonth><Day>1</Day><Day>3</Day></DaysOfMonth>"), "{out}");
    assert_eq!(out.matches("<Day>").count(), 2, "{out}");
}

#[test]
fn ranges_expand_to_all_values() {
    // Spec: `5-10` -> 5, 6, 7, 8, 9, 10.
    let (code, out, err) = render_cron("0 12 5-10 * *");
    assert_eq!(code, 0, "{err}");
    assert!(
        flat(&out).contains(
            "<DaysOfMonth><Day>5</Day><Day>6</Day><Day>7</Day><Day>8</Day><Day>9</Day><Day>10</Day></DaysOfMonth>"
        ),
        "{out}"
    );
}

// ------------------------------------------------------ time-of-day rows
// Spec time-of-day decomposition table: one row, one test. Assertions
// use time-of-day parts, which do not depend on the run date.

#[test]
fn star_minute_with_star_or_range_hour_is_one_trigger() {
    // Row 1: minute `*` (incl. step) + hour `*` or range -> one
    // trigger at hour Start:00, repeating every minute increment for
    // the hour range width.
    let (code, out, err) = render_cron("* * * * *");
    assert_eq!(code, 0, "{err}");
    assert_eq!(calendar_count(&out), 1, "{out}");
    assert_eq!(boundary_times(&out), ["00:00:00"], "{out}");
    assert!(out.contains("<Interval>PT1M</Interval>"), "{out}");
    assert!(out.contains("<Duration>PT24H</Duration>"), "{out}");

    let (_, out, _) = render_cron("*/15 9-17 * * *");
    assert_eq!(boundary_times(&out), ["09:00:00"], "{out}");
    assert!(out.contains("<Interval>PT15M</Interval>"), "{out}");
    assert!(out.contains("<Duration>PT9H</Duration>"), "{out}");
}

#[test]
fn star_minute_with_hour_list_or_step_expands_per_hour() {
    // Row 2: one trigger per hour value at h:00, repeating every
    // minute increment for one hour.
    let (code, out, err) = render_cron("* 3,5,6 * * *");
    assert_eq!(code, 0, "{err}");
    assert_eq!(boundary_times(&out), ["03:00:00", "05:00:00", "06:00:00"], "{out}");
    assert_eq!(out.matches("<Interval>PT1M</Interval>").count(), 3, "{out}");
    assert!(out.contains("<Duration>PT1H</Duration>"), "{out}");

    // Hour step `3/6` fires at 3, 9, 15, 21.
    let (_, out, _) = render_cron("* 3/6 * * *");
    assert_eq!(
        boundary_times(&out),
        ["03:00:00", "09:00:00", "15:00:00", "21:00:00"],
        "{out}"
    );
}

#[test]
fn minute_list_or_step_with_star_or_range_hour_expands_per_minute() {
    // Row 3: one trigger per minute value at hour Start:m, repeating
    // hourly for the hour range width.
    let (code, out, err) = render_cron("3,6 * * * *");
    assert_eq!(code, 0, "{err}");
    assert_eq!(boundary_times(&out), ["00:03:00", "00:06:00"], "{out}");
    assert_eq!(out.matches("<Interval>PT1H</Interval>").count(), 2, "{out}");
    assert!(out.contains("<Duration>PT24H</Duration>"), "{out}");

    // Minute step expands to its firing values (10, 30, 50).
    let (_, out, _) = render_cron("10-50/20 2-6 * * *");
    assert_eq!(boundary_times(&out), ["02:10:00", "02:30:00", "02:50:00"], "{out}");
    assert!(out.contains("<Duration>PT5H</Duration>"), "{out}");
}

#[test]
fn minute_range_or_step_with_hour_list_or_step_uses_minute_repetition() {
    // Row 4: one trigger per hour value at h:minute-Start, repeating
    // every minute increment for the minute range width.
    let (code, out, err) = render_cron("3-33/6 3,6 * * *");
    assert_eq!(code, 0, "{err}");
    assert_eq!(boundary_times(&out), ["03:03:00", "06:03:00"], "{out}");
    assert_eq!(out.matches("<Interval>PT6M</Interval>").count(), 2, "{out}");
    assert!(out.contains("<Duration>PT31M</Duration>"), "{out}");

    // Hour step `3/6` fires at 3, 9, 15, 21.
    let (_, out, _) = render_cron("3/6 3/6 * * *");
    assert_eq!(
        boundary_times(&out),
        ["03:03:00", "09:03:00", "15:03:00", "21:03:00"],
        "{out}"
    );
    assert!(out.contains("<Duration>PT57M</Duration>"), "{out}");
}

#[test]
fn minute_list_times_hour_list_is_all_combinations_without_repetition() {
    // Row 5: every hour x minute combination, no Repetition.
    let (code, out, err) = render_cron("0,30 9,21 * * *");
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        boundary_times(&out),
        ["09:00:00", "09:30:00", "21:00:00", "21:30:00"],
        "{out}"
    );
    assert_eq!(calendar_count(&out), 4, "{out}");
    assert!(!out.contains("<Repetition>"), "{out}");
}

// --------------------------------------------------------------- examples
// Spec examples below the time-of-day table.

#[test]
fn example_every_day_at_nine() {
    // `00 09 * * *` -> ScheduleByDay (DaysInterval=1) + StartBoundary
    // 09:00 on the run date, no repetition, one trigger.
    let out = render_cron_at("00 09 * * *", MIDNIGHT());
    assert_eq!(calendar_count(&out), 1, "{out}");
    assert!(out.contains("<DaysInterval>1</DaysInterval>"), "{out}");
    assert!(out.contains("<StartBoundary>2026-01-15T09:00:00</StartBoundary>"), "{out}");
    assert!(!out.contains("<Repetition>"), "{out}");
}

#[test]
fn example_four_daily_starts() {
    // `0,30 9,21 * * *` -> 9:00 / 9:30 / 21:00 / 21:30, 4 triggers.
    let out = render_cron_at("0,30 9,21 * * *", MIDNIGHT());
    assert_eq!(calendar_count(&out), 4, "{out}");
    assert_eq!(
        boundaries(triggers_of(&out)),
        [
            "2026-01-15T09:00:00",
            "2026-01-15T09:30:00",
            "2026-01-15T21:00:00",
            "2026-01-15T21:30:00",
        ],
        "{out}"
    );
}

#[test]
fn example_quarter_hour_across_business_hours() {
    // `*/15 9-17 * * *` -> ScheduleByDay + 09:00, Interval 15m,
    // Duration 9h, one trigger.
    let out = render_cron_at("*/15 9-17 * * *", MIDNIGHT());
    assert_eq!(calendar_count(&out), 1, "{out}");
    assert!(out.contains("<StartBoundary>2026-01-15T09:00:00</StartBoundary>"), "{out}");
    assert!(out.contains("<Interval>PT15M</Interval>"), "{out}");
    assert!(out.contains("<Duration>PT9H</Duration>"), "{out}");
}

// ------------------------------------------------- StartBoundary handling
// Spec: the date is the run date; a cron boundary at or before the
// run time is pushed one day forward; only cron triggers correct.

#[test]
fn start_boundary_date_is_the_run_date() {
    let out = render_cron_at("00 09 * * *", at(2026, 7, 4, 0, 0));
    assert!(out.contains("<StartBoundary>2026-07-04T09:00:00</StartBoundary>"), "{out}");
}

#[test]
fn start_boundary_in_the_past_is_pushed_one_day() {
    // Run time 10:00 is after the 09:00 boundary.
    let out = render_cron_at("00 09 * * *", at(2026, 1, 15, 10, 0));
    assert!(out.contains("<StartBoundary>2026-01-16T09:00:00</StartBoundary>"), "{out}");
}

#[test]
fn start_boundary_exactly_at_run_time_is_pushed() {
    // Spec: boundaries at or before the run time are corrected.
    let out = render_cron_at("00 09 * * *", at(2026, 1, 15, 9, 0));
    assert!(out.contains("<StartBoundary>2026-01-16T09:00:00</StartBoundary>"), "{out}");
}

#[test]
fn past_correction_does_not_apply_to_once_triggers() {
    // Spec: correction applies to cron only; a `once` boundary in the
    // past stays as written (startup/boot/now carry no boundary).
    let yaml = "- name: T\n  trigger: { type: once, value: \"2020-01-01 09:00\" }\n  action: { command: cmd.exe }\n";
    let defs = wintasks::def::parse_defs(yaml, "wintasks.yaml").expect("parse defs");
    let out = wintasks::render::render_output(&defs, at(2026, 1, 15, 10, 0)).expect("render");
    assert!(out.contains("<StartBoundary>2020-01-01T09:00:00</StartBoundary>"), "{out}");
}
