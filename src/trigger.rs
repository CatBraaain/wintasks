//! Trigger definitions converted to Task Scheduler trigger shapes.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};

use crate::cron::{CronSpec, DAY, DOW, Field, HOUR, MINUTE, MONTH};
use crate::def::{TriggerDef, TriggerKind};

#[derive(Debug, Clone, PartialEq)]
pub enum TriggerXml {
    Calendar(CalendarTrigger),
    Logon { delay: String },
    Boot { delay: String },
    Time { start_boundary: NaiveDateTime },
    Registration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarTrigger {
    pub start_boundary: NaiveDateTime,
    /// (interval, duration) as ISO 8601 durations.
    pub repetition: Option<(String, String)>,
    pub schedule: ScheduleKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleKind {
    Daily { days_interval: u32 },
    Weekly { days: Vec<u32> },
    Monthly { days: Vec<u32>, months: Vec<u32> },
    MonthlyDow { days: Vec<u32>, months: Vec<u32> },
}

pub fn build_triggers(
    trigger: &TriggerDef,
    today: NaiveDate,
    now: NaiveDateTime,
) -> Result<Vec<TriggerXml>, String> {
    match trigger.kind {
        TriggerKind::Cron => {
            let spec = CronSpec::parse(&trigger.value)?;
            Ok(build_calendar_triggers(&spec, today, now))
        }
        TriggerKind::Startup => Ok(vec![TriggerXml::Logon {
            delay: parse_delay(&trigger.value, "startup")?,
        }]),
        TriggerKind::Boot => Ok(vec![TriggerXml::Boot {
            delay: parse_delay(&trigger.value, "boot")?,
        }]),
        TriggerKind::Once => Ok(vec![TriggerXml::Time {
            start_boundary: parse_once(&trigger.value)?,
        }]),
        TriggerKind::Now => Ok(vec![TriggerXml::Registration]),
    }
}

fn build_calendar_triggers(
    spec: &CronSpec,
    today: NaiveDate,
    now: NaiveDateTime,
) -> Vec<TriggerXml> {
    select_schedules(spec)
        .into_iter()
        .flat_map(|schedule| {
            expand_times(spec, today)
                .into_iter()
                .map(move |(start, repetition)| {
                    // StartBoundary in the past is pushed one day forward,
                    // once, and only for cron triggers (spec).
                    let start = if start <= now {
                        start + Duration::days(1)
                    } else {
                        start
                    };
                    TriggerXml::Calendar(CalendarTrigger {
                        start_boundary: start,
                        repetition,
                        schedule: schedule.clone(),
                    })
                })
        })
        .collect()
}

/// Base trigger selection by day/month/weekday patterns, in the same
/// order as the old FromCronFormat (Weekly, MonthlyDOW, Monthly, Daily);
/// day+weekday constraints fire independently (OR semantics).
fn select_schedules(spec: &CronSpec) -> Vec<ScheduleKind> {
    let dow_every = spec.dow.is_every();
    let day_star = spec.day.is_star();
    let month_star = spec.month.is_star();
    // A star month still writes all twelve month elements (equivalent to no
    // constraint, but always valid against the Task Scheduler schema).
    let months = |spec: &CronSpec| -> Vec<u32> {
        if month_star {
            (1..=12).collect()
        } else {
            spec.month.values(MONTH.0, MONTH.1)
        }
    };
    let mut kinds = Vec::new();
    if day_star && month_star && !dow_every {
        kinds.push(ScheduleKind::Weekly {
            days: spec.dow.values(DOW.0, DOW.1),
        });
    }
    if !spec.dow.is_star() && (!day_star || !month_star) {
        kinds.push(ScheduleKind::MonthlyDow {
            days: spec.dow.values(DOW.0, DOW.1),
            months: months(spec),
        });
    }
    if !day_star || (!month_star && dow_every) {
        kinds.push(ScheduleKind::Monthly {
            days: spec.day.values(DAY.0, DAY.1),
            months: months(spec),
        });
    }
    if day_star && month_star && dow_every {
        kinds.push(ScheduleKind::Daily {
            days_interval: spec.day.incr(),
        });
    }
    kinds
}

/// Time-of-day expansion. Each element is (start datetime, repetition) and
/// mirrors FromCronFormat's ProcessCronTimes branch table.
fn expand_times(
    spec: &CronSpec,
    today: NaiveDate,
) -> Vec<(NaiveDateTime, Option<(String, String)>)> {
    let (min, hour) = (&spec.minute, &spec.hour);
    let at = |h: u32, m: u32| today.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap());
    let (min_lo, min_hi) = (MINUTE.0, MINUTE.1);
    let (hour_lo, hour_hi) = (HOUR.0, HOUR.1);
    let every_minute = |f: &Field| format!("PT{}M", f.incr());
    let hours_span = format!("PT{}H", hour.span(hour_lo, hour_hi));

    if min.is_star() && (hour.is_every() || hour.is_range()) {
        let start = at(hour.first(hour_lo), 0);
        vec![(start, Some((every_minute(min), hours_span)))]
    } else if min.is_star() {
        hour.values(hour_lo, hour_hi)
            .into_iter()
            .map(|h| (at(h, 0), Some((every_minute(min), "PT1H".to_string()))))
            .collect()
    } else if hour.is_every() || hour.is_range() {
        min.values(min_lo, min_hi)
            .into_iter()
            .map(|m| {
                (
                    at(hour.first(hour_lo), m),
                    Some(("PT1H".to_string(), hours_span.clone())),
                )
            })
            .collect()
    } else if min.is_range() || min.is_step() {
        hour.values(hour_lo, hour_hi)
            .into_iter()
            .map(|h| {
                (
                    at(h, min.first(min_lo)),
                    Some((
                        every_minute(min),
                        format!("PT{}M", min.span(min_lo, min_hi)),
                    )),
                )
            })
            .collect()
    } else {
        hour.values(hour_lo, hour_hi)
            .into_iter()
            .flat_map(|h| {
                min.values(min_lo, min_hi)
                    .into_iter()
                    .map(move |m| (at(h, m), None))
            })
            .collect()
    }
}

/// `HH:MM` or `HH:MM:SS` -> ISO 8601 duration (e.g. PT1H30M).
fn parse_delay(value: &str, kind: &str) -> Result<String, String> {
    let parts: Vec<&str> = value.split(':').collect();
    let invalid = || format!("invalid {kind} trigger value `{value}`: expected HH:MM or HH:MM:SS");
    let two_digits = |s: &str| -> Option<u32> {
        if s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    let (h, m, s) = match parts.as_slice() {
        [h, m] => (two_digits(h), two_digits(m), Some(0)),
        [h, m, s] => (two_digits(h), two_digits(m), two_digits(s)),
        _ => (None, None, None),
    };
    let (h, m, s) = match (h, m, s) {
        (Some(h), Some(m), Some(s)) if m < 60 && s < 60 => (h, m, s),
        _ => return Err(invalid()),
    };
    let mut out = String::from("PT");
    if h > 0 {
        out.push_str(&format!("{h}H"));
    }
    if m > 0 {
        out.push_str(&format!("{m}M"));
    }
    if s > 0 {
        out.push_str(&format!("{s}S"));
    }
    if out == "PT" {
        out.push_str("0S");
    }
    Ok(out)
}

/// `YYYY-MM-DD` or `YYYY-MM-DD HH:MM[:SS]`; omitted time parts are zero.
fn parse_once(value: &str) -> Result<NaiveDateTime, String> {
    let invalid = || {
        format!(
            "invalid once trigger value `{value}`: expected YYYY-MM-DD or YYYY-MM-DD HH:MM[:SS]"
        )
    };
    let (date_part, time_part) = match value.split_once(' ') {
        Some((d, t)) => (d, Some(t)),
        None => (value, None),
    };
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").map_err(|_| invalid())?;
    let time = match time_part {
        None => NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        Some(t) => {
            let seconds: Vec<&str> = t.split(':').collect();
            match seconds.as_slice() {
                [h, m] => {
                    let (h, m): (u32, u32) = (fast_two_digits(h)?, fast_two_digits(m)?);
                    NaiveTime::from_hms_opt(h, m, 0).ok_or_else(invalid)?
                }
                [h, m, s] => {
                    let (h, m, s): (u32, u32, u32) = (
                        fast_two_digits(h)?,
                        fast_two_digits(m)?,
                        fast_two_digits(s)?,
                    );
                    NaiveTime::from_hms_opt(h, m, s).ok_or_else(invalid)?
                }
                _ => return Err(invalid()),
            }
        }
    };
    Ok(date.and_time(time))
}

fn fast_two_digits(s: &str) -> Result<u32, String> {
    if s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit()) {
        Ok(s.parse().unwrap())
    } else {
        Err(format!("invalid time component `{s}`: expected two digits"))
    }
}

pub fn weekday_element(dow: u32) -> &'static str {
    const NAMES: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    NAMES[dow as usize]
}

pub fn month_element(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    NAMES[(month - 1) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Timelike};

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        date(y, mo, d).and_hms_opt(h, mi, 0).unwrap()
    }

    fn cron(value: &str) -> TriggerDef {
        TriggerDef {
            kind: TriggerKind::Cron,
            value: value.to_string(),
        }
    }

    fn calendars(triggers: &[TriggerXml]) -> Vec<&CalendarTrigger> {
        triggers
            .iter()
            .filter_map(|t| match t {
                TriggerXml::Calendar(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn daily_every_day_at_nine() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("00 09 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].start_boundary, dt(2026, 1, 15, 9, 0));
        assert_eq!(cals[0].repetition, None);
        assert_eq!(cals[0].schedule, ScheduleKind::Daily { days_interval: 1 });
    }

    #[test]
    fn minute_step_with_hour_range_uses_repetition() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("*/15 9-17 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].start_boundary, dt(2026, 1, 15, 9, 0));
        assert_eq!(
            cals[0].repetition,
            Some(("PT15M".to_string(), "PT9H".to_string()))
        );
    }

    #[test]
    fn every_minute_every_hour_is_one_trigger_with_day_span() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("* * * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        // StartBoundary 00:00 equals `now` 00:00, so the past-start
        // correction pushes it one day forward (spec: <= now).
        assert_eq!(cals[0].start_boundary, dt(2026, 1, 16, 0, 0));
        assert_eq!(
            cals[0].repetition,
            Some(("PT1M".to_string(), "PT24H".to_string()))
        );
    }

    #[test]
    fn hour_list_with_every_minute_expands_per_hour() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("* 3,5,6 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        let starts: Vec<_> = cals
            .iter()
            .map(|c| c.start_boundary.time().hour())
            .collect();
        assert_eq!(starts, vec![3, 5, 6]);
        assert!(
            cals.iter()
                .all(|c| c.repetition == Some(("PT1M".to_string(), "PT1H".to_string())))
        );
    }

    #[test]
    fn minute_list_with_any_hour_expands_per_minute() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("3,6 * * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        let minutes: Vec<_> = cals
            .iter()
            .map(|c| c.start_boundary.time().minute())
            .collect();
        assert_eq!(minutes, vec![3, 6]);
        assert!(
            cals.iter()
                .all(|c| c.repetition.as_ref().unwrap().0 == "PT1H")
        );
        assert!(
            cals.iter()
                .all(|c| c.repetition.as_ref().unwrap().1 == "PT24H")
        );
    }

    #[test]
    fn minute_range_with_hour_list_uses_minute_repetition() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("3-33/6 3,6 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        let starts: Vec<String> = cals
            .iter()
            .map(|c| c.start_boundary.format("%H:%M").to_string())
            .collect();
        assert_eq!(starts, vec!["03:03", "06:03"]);
        assert!(
            cals.iter()
                .all(|c| c.repetition == Some(("PT6M".to_string(), "PT31M".to_string())))
        );
    }

    #[test]
    fn minute_list_and_hour_list_expand_all_combinations() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("0,30 9,21 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        let starts: Vec<String> = cals
            .iter()
            .map(|c| c.start_boundary.format("%H:%M").to_string())
            .collect();
        assert_eq!(starts, vec!["09:00", "09:30", "21:00", "21:30"]);
        assert!(cals.iter().all(|c| c.repetition.is_none()));
    }

    #[test]
    fn day_step_becomes_daily_interval() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("0 12 */2 * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals[0].schedule, ScheduleKind::Daily { days_interval: 2 });
    }

    #[test]
    fn weekday_list_becomes_weekly() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("0 12 * * MON,WED,FRI"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        assert_eq!(
            cals[0].schedule,
            ScheduleKind::Weekly {
                days: vec![1, 3, 5]
            }
        );
    }

    #[test]
    fn month_with_star_day_becomes_monthly_all_days() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("0 12 * JAN *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        assert_eq!(
            cals[0].schedule,
            ScheduleKind::Monthly {
                days: (1..=31).collect::<Vec<u32>>(),
                months: vec![1],
            }
        );
    }

    #[test]
    fn day_list_becomes_monthly() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("0 12 1,15 * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(
            cals[0].schedule,
            ScheduleKind::Monthly {
                days: vec![1, 15],
                months: (1..=12).collect::<Vec<u32>>()
            }
        );
    }

    #[test]
    fn day_and_weekday_both_set_produce_two_triggers() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("0 12 1 * MON"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 2);
        assert_eq!(
            cals[0].schedule,
            ScheduleKind::MonthlyDow {
                days: vec![1],
                months: (1..=12).collect::<Vec<u32>>()
            }
        );
        assert_eq!(
            cals[1].schedule,
            ScheduleKind::Monthly {
                days: vec![1],
                months: (1..=12).collect::<Vec<u32>>()
            }
        );
    }

    #[test]
    fn weekday_and_month_becomes_monthly_dow() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("0 12 * JAN MON"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        assert_eq!(
            cals[0].schedule,
            ScheduleKind::MonthlyDow {
                days: vec![1],
                months: vec![1]
            }
        );
    }

    #[test]
    fn start_boundary_in_past_is_pushed_one_day() {
        let today = date(2026, 1, 15);
        // now is 10:00; the 09:00 start is in the past -> Jan 16.
        let triggers = build_triggers(&cron("00 09 * * *"), today, dt(2026, 1, 15, 10, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals[0].start_boundary, dt(2026, 1, 16, 9, 0));
    }

    #[test]
    fn start_boundary_equal_to_now_is_pushed_one_day() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("00 09 * * *"), today, dt(2026, 1, 15, 9, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals[0].start_boundary, dt(2026, 1, 16, 9, 0));
    }

    #[test]
    fn past_correction_applies_only_to_cron() {
        let today = date(2026, 1, 15);
        let now = dt(2026, 1, 15, 12, 0);
        let once = build_triggers(
            &TriggerDef {
                kind: TriggerKind::Once,
                value: "2020-01-01 09:00".to_string(),
            },
            today,
            now,
        )
        .unwrap();
        match &once[0] {
            TriggerXml::Time { start_boundary } => {
                // Far in the past and left as-is: no correction for once.
                assert_eq!(*start_boundary, dt(2020, 1, 1, 9, 0));
            }
            _ => panic!("expected Time trigger"),
        }
    }

    #[test]
    fn every_minute_with_hour_range_starts_at_range_beginning() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("* 2-6 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].start_boundary, dt(2026, 1, 15, 2, 0));
        assert_eq!(
            cals[0].repetition,
            Some(("PT1M".to_string(), "PT5H".to_string()))
        );
    }

    #[test]
    fn minute_range_with_hour_range_expands_every_minute_value() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("3-33 3-5 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        // Inefficient but faithful to FromCronFormat: one trigger per minute
        // value (31), each repeating hourly for the hour-range span.
        assert_eq!(cals.len(), 31);
        assert_eq!(cals[0].start_boundary, dt(2026, 1, 15, 3, 3));
        assert_eq!(
            cals[0].repetition,
            Some(("PT1H".to_string(), "PT3H".to_string()))
        );
    }

    #[test]
    fn minute_step_and_hour_step_use_minute_repetition_per_hour() {
        let today = date(2026, 1, 15);
        let triggers =
            build_triggers(&cron("3/6 3/6 * * *"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        // Both fields are steps (IsIncr in FromCronFormat), so the else-if
        // chain takes branch 4: one trigger per hour value (3,9,15,21), each
        // repeating every 6 minutes for the minute-range span (3-59 -> 57).
        assert_eq!(cals.len(), 4);
        let starts: Vec<String> = cals
            .iter()
            .map(|c| c.start_boundary.format("%H:%M").to_string())
            .collect();
        assert_eq!(starts, vec!["03:03", "09:03", "15:03", "21:03"]);
        assert!(
            cals.iter()
                .all(|c| c.repetition == Some(("PT6M".to_string(), "PT57M".to_string())))
        );
    }

    #[test]
    fn full_dow_range_is_treated_as_every_day() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(&cron("0 12 * * 0-6"), today, dt(2026, 1, 15, 0, 0)).unwrap();
        let cals = calendars(&triggers);
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].schedule, ScheduleKind::Daily { days_interval: 1 });
    }

    #[test]
    fn startup_delay_converts_to_iso_duration() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(
            &TriggerDef {
                kind: TriggerKind::Startup,
                value: "01:30".to_string(),
            },
            today,
            dt(2026, 1, 15, 0, 0),
        )
        .unwrap();
        assert_eq!(
            triggers[0],
            TriggerXml::Logon {
                delay: "PT1H30M".to_string()
            }
        );
    }

    #[test]
    fn boot_delay_with_seconds() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(
            &TriggerDef {
                kind: TriggerKind::Boot,
                value: "00:00:30".to_string(),
            },
            today,
            dt(2026, 1, 15, 0, 0),
        )
        .unwrap();
        assert_eq!(
            triggers[0],
            TriggerXml::Boot {
                delay: "PT30S".to_string()
            }
        );
    }

    #[test]
    fn zero_delay_formats_as_pt0s() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(
            &TriggerDef {
                kind: TriggerKind::Boot,
                value: "00:00".to_string(),
            },
            today,
            dt(2026, 1, 15, 0, 0),
        )
        .unwrap();
        assert_eq!(
            triggers[0],
            TriggerXml::Boot {
                delay: "PT0S".to_string()
            }
        );
    }

    #[test]
    fn rejects_invalid_delays() {
        let today = date(2026, 1, 15);
        for bad in ["9:00", "0130", "01:60", "01:00:60", "01"] {
            let r = build_triggers(
                &TriggerDef {
                    kind: TriggerKind::Startup,
                    value: bad.to_string(),
                },
                today,
                dt(2026, 1, 15, 0, 0),
            );
            assert!(r.is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn once_parses_date_only_as_midnight() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(
            &TriggerDef {
                kind: TriggerKind::Once,
                value: "2000-01-01".to_string(),
            },
            today,
            dt(2026, 1, 15, 0, 0),
        )
        .unwrap();
        assert_eq!(
            triggers[0],
            TriggerXml::Time {
                start_boundary: dt(2000, 1, 1, 0, 0)
            }
        );
    }

    #[test]
    fn once_parses_datetime_with_seconds() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(
            &TriggerDef {
                kind: TriggerKind::Once,
                value: "2030-06-01 07:45:30".to_string(),
            },
            today,
            dt(2026, 1, 15, 0, 0),
        )
        .unwrap();
        let expected = date(2030, 6, 1).and_hms_opt(7, 45, 30).unwrap();
        assert_eq!(
            triggers[0],
            TriggerXml::Time {
                start_boundary: expected
            }
        );
    }

    #[test]
    fn rejects_invalid_once_values() {
        let today = date(2026, 1, 15);
        for bad in [
            "2000-13-01",
            "01-01-2000",
            "2000-01-01T09:00",
            "2000-01-01 9:00",
        ] {
            let r = build_triggers(
                &TriggerDef {
                    kind: TriggerKind::Once,
                    value: bad.to_string(),
                },
                today,
                dt(2026, 1, 15, 0, 0),
            );
            assert!(r.is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn now_trigger_is_registration() {
        let today = date(2026, 1, 15);
        let triggers = build_triggers(
            &TriggerDef {
                kind: TriggerKind::Now,
                value: "dummy".to_string(),
            },
            today,
            dt(2026, 1, 15, 0, 0),
        )
        .unwrap();
        assert_eq!(triggers[0], TriggerXml::Registration);
    }
}
