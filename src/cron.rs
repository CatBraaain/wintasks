//! Cron expression parsing for trigger decomposition.
//!
//! The spec's trigger decomposition table needs the *structure* of each field
//! (star / range / step / list), which `cron` crate's public API does not
//! expose (it only provides expanded value sets). So syntax is parsed here,
//! and the reconstructed expression is validated through `cron` crate
//! (with seconds `0` prepended and weekday values remapped 0-6 -> 1-7,
//! because the crate requires 6 fields and numbers weekdays 1-7 with
//! Sunday=1) as a final semantic check.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    /// `*`, `?`, `*/n`, or a step applied to the full range (e.g. `0-59/5`).
    Star { incr: u32 },
    /// `n-m`, `n-m/n`, `n/n` (end defaults to the field maximum).
    Range { start: u32, end: u32, incr: u32 },
    /// Comma list or a single value; sorted, deduplicated.
    List(Vec<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Star,
    Every, // star without a step
    Range, // n-m without a step
    Step,  // range or n/n with a step
    List,
}

impl Field {
    fn kind(&self) -> FieldKind {
        match *self {
            Field::Star { incr: 1 } => FieldKind::Every,
            Field::Star { .. } => FieldKind::Star,
            Field::Range { incr: 1, .. } => FieldKind::Range,
            Field::Range { .. } => FieldKind::Step,
            Field::List(_) => FieldKind::List,
        }
    }

    pub fn is_star(&self) -> bool {
        matches!(self.kind(), FieldKind::Star | FieldKind::Every)
    }

    pub fn is_every(&self) -> bool {
        self.kind() == FieldKind::Every
    }

    pub fn is_range(&self) -> bool {
        self.kind() == FieldKind::Range
    }

    pub fn is_step(&self) -> bool {
        self.kind() == FieldKind::Step
    }

    pub fn is_list(&self) -> bool {
        self.kind() == FieldKind::List
    }

    /// Expanded values: list values as-is; ranges and stars enumerated by
    /// their increment.
    pub fn values(&self, min: u32, max: u32) -> Vec<u32> {
        match *self {
            Field::Star { incr } => (min..=max).step_by(incr as usize).collect(),
            Field::Range { start, end, incr } => (start..=end).step_by(incr as usize).collect(),
            Field::List(ref vs) => vs.clone(),
        }
    }

    /// First value of the expansion.
    pub fn first(&self, min: u32) -> u32 {
        match *self {
            Field::Star { .. } => min,
            Field::Range { start, .. } => start,
            Field::List(ref vs) => vs[0],
        }
    }

    /// Span used for repetition duration: `end - start + 1` (ignores the
    /// step, like FromCronFormat's Duration).
    pub fn span(&self, min: u32, max: u32) -> u32 {
        match *self {
            Field::Star { .. } => max - min + 1,
            Field::Range { start, end, .. } => end - start + 1,
            Field::List(ref vs) => vs.last().unwrap() - vs[0] + 1,
        }
    }

    /// Increment for repetition interval; 1 when no step is given.
    pub fn incr(&self) -> u32 {
        match *self {
            Field::Star { incr } | Field::Range { incr, .. } => incr,
            Field::List(_) => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CronSpec {
    pub minute: Field,
    pub hour: Field,
    pub day: Field,
    pub month: Field,
    pub dow: Field,
}

pub const MINUTE: (u32, u32) = (0, 59);
pub const HOUR: (u32, u32) = (0, 23);
pub const DAY: (u32, u32) = (1, 31);
pub const MONTH: (u32, u32) = (1, 12);
// Spec weekday is 0-6 with Sunday=0 (cron crate uses 1-7; remapped when
// validating through the crate).
pub const DOW: (u32, u32) = (0, 6);

impl CronSpec {
    pub fn parse(expr: &str) -> Result<CronSpec, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!(
                "invalid cron expression `{expr}`: expected 5 fields (minute hour day month weekday), got {}",
                fields.len()
            ));
        }
        let spec = CronSpec {
            minute: parse_field(fields[0], "minute", MINUTE, &[], false)?,
            hour: parse_field(fields[1], "hour", HOUR, &[], false)?,
            day: parse_field(fields[2], "day", DAY, &[], true)?,
            month: parse_field(fields[3], "month", MONTH, &month_names(), false)?,
            dow: parse_field(fields[4], "weekday", DOW, &dow_names(), true)?,
        };
        validate_with_cron_crate(&spec)
            .map_err(|e| format!("invalid cron expression `{expr}`: {e}"))?;
        Ok(spec)
    }
}

fn month_names() -> Vec<(&'static str, u32)> {
    [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ]
    .iter()
    .enumerate()
    .map(|(i, n)| (*n, i as u32 + 1))
    .collect()
}

// Values follow the spec weekday numbering: 0=Sunday.
fn dow_names() -> Vec<(&'static str, u32)> {
    ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"]
        .iter()
        .enumerate()
        .map(|(i, n)| (*n, i as u32))
        .collect()
}

fn parse_field(
    text: &str,
    name: &str,
    (min, max): (u32, u32),
    names: &[(&'static str, u32)],
    allows_question: bool,
) -> Result<Field, String> {
    // Only day and weekday accept `?`, and only as the whole field.
    if text == "?" {
        if !allows_question {
            return Err(format!("`?` is not allowed in the {name} field"));
        }
        return Ok(Field::Star { incr: 1 });
    }

    // Reject step inside list elements (e.g. `1,3/2`) by requiring `/` to
    // appear at most once and only in a whole-field (non-comma) expression.
    let comma_count = text.matches(',').count();
    let slash_count = text.matches('/').count();
    if slash_count > 1 || (slash_count == 1 && comma_count > 0) {
        return Err(format!(
            "invalid {name} field `{text}`: steps cannot be mixed with lists"
        ));
    }

    if comma_count > 0 {
        let mut values = Vec::new();
        for item in text.split(',') {
            // Each list element is a single value or a range, never a step.
            let (lo, hi) = parse_value_or_range(item, name, min, max, names)?;
            values.extend(lo..=hi);
        }
        values.sort_unstable();
        values.dedup();
        return Ok(Field::List(values));
    }

    let (body, incr, has_step) = match text.split_once('/') {
        Some((body, step)) => {
            let incr: u32 = step
                .parse()
                .map_err(|_| format!("invalid {name} field `{text}`: step must be a number"))?;
            if incr == 0 {
                return Err(format!("invalid {name} field `{text}`: step must be >= 1"));
            }
            (body, incr, true)
        }
        None => (text, 1, false),
    };

    // Month and weekday steps have no Task Scheduler representation
    // (same limitation as the old FromCronFormat).
    if incr > 1 && (name == "month" || name == "weekday") {
        return Err(format!(
            "invalid {name} field `{text}`: steps are not supported for month and weekday"
        ));
    }

    if body == "*" {
        return Ok(Field::Star { incr });
    }

    let (start, end) = parse_value_or_range(body, name, min, max, names)?;
    if !body.contains('-') {
        // Single value: `n` is a one-element list; `n/n` runs from n to the
        // field maximum with step n (FromCronFormat's IsIncr form).
        if has_step {
            return Ok(Field::Range {
                start,
                end: max,
                incr,
            });
        }
        return Ok(Field::List(vec![start]));
    }
    if start == end {
        // `n-n` degenerates to a single value, like FromCronFormat's IsList.
        return Ok(Field::List(vec![start]));
    }

    // A range spanning the whole field is equivalent to `*` (FromCronFormat
    // rewrites it to `*` before selecting the trigger kind).
    if start == min && end == max {
        return Ok(Field::Star { incr });
    }
    Ok(Field::Range { start, end, incr })
}

/// Parses `n`, `n-m`, or a name-based range; returns (lo, hi) with lo <= hi.
fn parse_value_or_range(
    text: &str,
    name: &str,
    min: u32,
    max: u32,
    names: &[(&'static str, u32)],
) -> Result<(u32, u32), String> {
    let parse_one = |token: &str| -> Result<u32, String> {
        let upper = token.to_ascii_uppercase();
        if let Some((_, v)) = names.iter().find(|(n, _)| *n == upper) {
            return Ok(*v);
        }
        let v: u32 = token
            .parse()
            .map_err(|_| format!("invalid {name} field value `{token}`"))?;
        if v < min || v > max {
            return Err(format!(
                "invalid {name} field value `{token}`: must be {min}-{max}"
            ));
        }
        Ok(v)
    };

    match text.split_once('-') {
        Some((lo, hi)) => {
            let lo = parse_one(lo)?;
            let hi = parse_one(hi)?;
            if lo > hi {
                return Err(format!("invalid {name} field `{text}`: range is reversed"));
            }
            Ok((lo, hi))
        }
        None => {
            let v = parse_one(text)?;
            Ok((v, v))
        }
    }
}

fn validate_with_cron_crate(spec: &CronSpec) -> Result<(), String> {
    let dow_field = |f: &Field| -> String {
        // Spec 0-6 (Sunday=0) -> crate 1-7 (Sunday=1).
        let remap = |v: u32| v + 1;
        match *f {
            Field::Star { incr: 1 } => "*".to_string(),
            Field::Star { incr } => format!("*/{incr}"),
            Field::Range {
                start,
                end,
                incr: 1,
            } => format!("{}-{}", remap(start), remap(end)),
            Field::Range { start, end, incr } => {
                format!("{}-{}/{}", remap(start), remap(end), incr)
            }
            Field::List(ref vs) => vs
                .iter()
                .map(|v| remap(*v).to_string())
                .collect::<Vec<_>>()
                .join(","),
        }
    };
    let plain = |f: &Field| -> String {
        match *f {
            Field::Star { incr: 1 } => "*".to_string(),
            Field::Star { incr } => format!("*/{incr}"),
            Field::Range {
                start,
                end,
                incr: 1,
            } => format!("{start}-{end}"),
            Field::Range { start, end, incr } => format!("{start}-{end}/{incr}"),
            Field::List(ref vs) => vs
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(","),
        }
    };
    let expr = format!(
        "0 {} {} {} {} {}",
        plain(&spec.minute),
        plain(&spec.hour),
        plain(&spec.day),
        plain(&spec.month),
        dow_field(&spec.dow)
    );
    cron::Schedule::from_str(&expr).map_err(|e| e.to_string())?;
    Ok(())
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Field::Star { incr: 1 } => write!(f, "*"),
            Field::Star { incr } => write!(f, "*/{incr}"),
            Field::Range {
                start,
                end,
                incr: 1,
            } => write!(f, "{start}-{end}"),
            Field::Range { start, end, incr } => write!(f, "{start}-{end}/{incr}"),
            Field::List(vs) => {
                let joined = vs
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                write!(f, "{joined}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minute(text: &str) -> Field {
        parse_field(text, "minute", MINUTE, &[], false).unwrap()
    }

    fn minute_err(text: &str) -> String {
        parse_field(text, "minute", MINUTE, &[], false).unwrap_err()
    }

    fn dow(text: &str) -> Field {
        parse_field(text, "weekday", DOW, &dow_names(), true).unwrap()
    }

    fn dow_err(text: &str) -> String {
        parse_field(text, "weekday", DOW, &dow_names(), true).unwrap_err()
    }

    fn month(text: &str) -> Field {
        parse_field(text, "month", MONTH, &month_names(), false).unwrap()
    }

    fn month_err(text: &str) -> String {
        parse_field(text, "month", MONTH, &month_names(), false).unwrap_err()
    }

    #[test]
    fn star_and_question_normalize_to_star() {
        assert_eq!(minute("*"), Field::Star { incr: 1 });
        assert_eq!(dow("?"), Field::Star { incr: 1 });
    }

    #[test]
    fn full_range_normalizes_to_star() {
        assert_eq!(minute("0-59"), Field::Star { incr: 1 });
        assert_eq!(dow("0-6"), Field::Star { incr: 1 });
        assert_eq!(minute("0-59/5"), Field::Star { incr: 5 });
    }

    #[test]
    fn single_value_and_degenerate_range_become_list() {
        assert_eq!(minute("5"), Field::List(vec![5]));
        assert_eq!(minute("5-5"), Field::List(vec![5]));
    }

    #[test]
    fn n_slash_n_extends_to_field_max() {
        assert_eq!(
            minute("30/10"),
            Field::Range {
                start: 30,
                end: 59,
                incr: 10
            }
        );
    }

    #[test]
    fn list_expands_ranges_and_sorts() {
        assert_eq!(minute("5,1-3,9"), Field::List(vec![1, 2, 3, 5, 9]));
    }

    #[test]
    fn names_are_case_insensitive_and_mixed_with_numbers() {
        assert_eq!(dow("sun,1,Fri"), Field::List(vec![0, 1, 5]));
        assert_eq!(month("Jan-dec"), Field::Star { incr: 1 });
    }

    #[test]
    fn rejects_question_in_non_day_fields() {
        assert!(parse_field("?", "minute", MINUTE, &[], false).is_err());
        assert!(parse_field("?,5", "weekday", DOW, &dow_names(), true).is_err());
    }

    #[test]
    fn rejects_month_and_dow_steps() {
        assert!(month_err("*/5").contains("not supported"));
        assert!(dow_err("MON/3").contains("not supported"));
        assert!(month_err("1-12/2").contains("not supported"));
    }

    #[test]
    fn rejects_dow_seven() {
        assert!(dow_err("7").contains("0-6"));
    }

    #[test]
    fn rejects_list_step_mix() {
        let f = parse_field("1,3/2", "minute", MINUTE, &[], false);
        assert!(f.unwrap_err().contains("cannot be mixed"));
    }

    #[test]
    fn rejects_reversed_and_out_of_range() {
        assert!(minute_err("5-1").contains("reversed"));
        assert!(minute_err("60").contains("0-59"));
        assert!(minute_err("*/0").contains(">= 1"));
    }

    #[test]
    fn rejects_quartz_extensions() {
        assert!(minute_err("L").contains("invalid"));
        assert!(minute_err("15W").contains("invalid"));
        assert!(CronSpec::parse("@daily").is_err());
        assert!(CronSpec::parse("* * * * * *").is_err());
    }

    #[test]
    fn parses_five_field_expression_with_cron_crate_validation() {
        let spec = CronSpec::parse("00 09 * * *").unwrap();
        assert_eq!(spec.minute, Field::List(vec![0]));
        assert_eq!(spec.hour, Field::List(vec![9]));
        assert_eq!(spec.day, Field::Star { incr: 1 });
        assert_eq!(spec.dow, Field::Star { incr: 1 });
    }

    #[test]
    fn cron_crate_validation_maps_sunday_zero() {
        // dow 0 (Sunday) must survive the 1-7 remap used for validation.
        CronSpec::parse("0 12 * * 0").unwrap();
        CronSpec::parse("0 12 * * sun").unwrap();
        CronSpec::parse("*/15 9-17 * * MON-FRI").unwrap();
    }

    #[test]
    fn field_helpers() {
        let star = Field::Star { incr: 1 };
        assert!(star.is_star() && star.is_every());
        let step = Field::Range {
            start: 3,
            end: 33,
            incr: 6,
        };
        assert!(step.is_step() && !step.is_range());
        assert_eq!(step.values(0, 59), vec![3, 9, 15, 21, 27, 33]);
        assert_eq!(step.span(0, 59), 31);
        assert_eq!(star.span(0, 23), 24);
        assert_eq!(star.first(0), 0);
    }
}
