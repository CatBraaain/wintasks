//! Conversion test cases extracted from SPEC.md.
//!
//! SPEC.md owns the test data. The `## 変換テストケース` section holds
//! tables whose rows are one conversion case each, and the `## XML 生成`
//! section holds the documented example as yaml/xml fences. This module
//! extracts both with a line-based parser and hands them to the tests.
//!
//! The parser accepts exactly the documented shape and panics on anything
//! else, so a malformed SPEC fails the tests instead of silently skipping
//! cases.

use std::sync::LazyLock;

/// One conversion case: minimal wrapped task definitions rendered at a
/// fixed local time must produce output containing `expected`, compared
/// after whitespace-only text between tags is removed on both sides.
pub struct SpecCase {
    pub name: String,
    pub yaml: String,
    pub now: (i32, u32, u32, u32, u32),
    pub expected: String,
}

/// All rows of both conversion tables, in document order.
pub fn cases() -> &'static [SpecCase] {
    static CASES: LazyLock<Vec<SpecCase>> = LazyLock::new(extract_cases);
    &CASES
}

/// The documented example from the `## XML 生成` section: its yaml fence
/// is the input and its xml fence is the expected document.
pub struct ExampleCase {
    pub yaml: String,
    pub expected: String,
}

pub fn example() -> &'static ExampleCase {
    static EXAMPLE: LazyLock<ExampleCase> = LazyLock::new(extract_example);
    &EXAMPLE
}

/// Removes whitespace-only text between tags so comparisons can ignore
/// indentation. Text inside elements is untouched.
pub fn compact(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut chars = xml.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c != '>' {
            continue;
        }
        let mut whitespace = String::new();
        while let Some(&next) = chars.peek() {
            if !next.is_whitespace() {
                break;
            }
            whitespace.push(next);
            chars.next();
        }
        if chars.peek() != Some(&'<') {
            out.push_str(&whitespace);
        }
    }
    out
}

fn extract_cases() -> Vec<SpecCase> {
    let lines: Vec<&str> = include_str!("../../SPEC.md").lines().collect();
    let section = section_lines(&lines, "## 変換テストケース");
    let mut cases = Vec::new();
    for table in tables(&section) {
        let header = &table[0];
        match header[0].as_str() {
            "type" => {
                check_header(header, &["type", "value", "実行時刻", "期待 XML"]);
                for row in &table[2..] {
                    check_width(row, header.len());
                    cases.push(trigger_case(row));
                }
            }
            "command" => {
                check_header(
                    header,
                    &["command", "args", "working_directory", "期待 XML"],
                );
                for row in &table[2..] {
                    check_width(row, header.len());
                    cases.push(action_case(row));
                }
            }
            other => panic!("SPEC.md 変換テストケース: unknown table `{other}`"),
        }
    }
    if cases.is_empty() {
        panic!("SPEC.md 変換テストケース: no cases found");
    }
    cases
}

fn trigger_case(row: &[String]) -> SpecCase {
    let kind = code_cell(&row[0]);
    let value = code_cell(&row[1]);
    let now_cell = row[2].trim();
    if kind == "cron" && now_cell.is_empty() {
        panic!("SPEC.md 変換テストケース: cron row `{value}` lacks a run time");
    }
    let now = if now_cell.is_empty() {
        // Other trigger kinds do not depend on the run time.
        (2000, 1, 1, 0, 0)
    } else {
        parse_now(now_cell)
    };
    SpecCase {
        name: format!("{kind} {value}"),
        yaml: format!(
            "- name: sample\n  trigger: {{ type: {kind}, value: \"{value}\" }}\n  action: {{ command: cmd.exe }}\n"
        ),
        now,
        expected: code_cell(&row[3]).to_string(),
    }
}

fn action_case(row: &[String]) -> SpecCase {
    let command = code_cell(&row[0]);
    let mut action = format!("{{ command: {command}");
    let args = code_cell(&row[1]);
    if !args.is_empty() {
        action.push_str(&format!(", args: {args}"));
    }
    let working_directory = code_cell(&row[2]);
    if !working_directory.is_empty() {
        action.push_str(&format!(", working_directory: {working_directory}"));
    }
    action.push_str(" }");
    SpecCase {
        name: format!("action {action}"),
        yaml: format!(
            "- name: sample\n  trigger: {{ type: now, value: \"x\" }}\n  action: {action}\n"
        ),
        now: (2000, 1, 1, 0, 0),
        expected: code_cell(&row[3]).to_string(),
    }
}

/// Strips one pair of surrounding backticks, the markdown inline-code
/// formatting used by the table cells.
fn code_cell(cell: &str) -> &str {
    let trimmed = cell.trim();
    trimmed
        .strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or(trimmed)
}

fn check_header(header: &[String], wanted: &[&str]) {
    let matches =
        header.len() == wanted.len() && header.iter().zip(wanted).all(|(cell, want)| cell == want);
    if !matches {
        panic!("SPEC.md 変換テストケース: unexpected table header {header:?}");
    }
}

fn check_width(row: &[String], width: usize) {
    if row.len() != width {
        panic!("SPEC.md 変換テストケース: row width {width}: {row:?}");
    }
}

fn extract_example() -> ExampleCase {
    let lines: Vec<&str> = include_str!("../../SPEC.md").lines().collect();
    let section = section_lines(&lines, "## XML 生成");
    let (yaml, after_yaml) = fence_block(&section, "yaml", 0);
    let (xml, _) = fence_block(&section, "xml", after_yaml);
    ExampleCase {
        yaml,
        expected: xml,
    }
}

fn section_lines<'a>(lines: &[&'a str], heading: &str) -> Vec<&'a str> {
    let start = lines
        .iter()
        .position(|line| *line == heading)
        .unwrap_or_else(|| panic!("SPEC.md: heading `{heading}` not found"));
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.starts_with("## "))
        .map_or(lines.len(), |offset| start + 1 + offset);
    lines[start..end].to_vec()
}

/// Returns the content of the first fence labeled `info` at or after
/// `from`, together with the line index just past its closing fence.
fn fence_block(lines: &[&str], info: &str, from: usize) -> (String, usize) {
    let open = format!("```{info}");
    let start = lines[from..]
        .iter()
        .position(|line| *line == open)
        .unwrap_or_else(|| panic!("SPEC.md: fence `{open}` not found"))
        + from;
    let end = lines[start + 1..]
        .iter()
        .position(|line| *line == "```")
        .unwrap_or_else(|| panic!("SPEC.md: fence `{open}` is not closed"))
        + start
        + 1;
    let mut content = lines[start + 1..end].join("\n");
    content.push('\n');
    (content, end)
}

/// Groups consecutive `|` lines into tables of trimmed cells. The second
/// row must be the separator row.
fn tables(lines: &[&str]) -> Vec<Vec<Vec<String>>> {
    let mut tables = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    for line in lines {
        if line.trim_start().starts_with('|') {
            rows.push(row_cells(line));
        } else if !rows.is_empty() {
            tables.push(std::mem::take(&mut rows));
        }
    }
    if !rows.is_empty() {
        tables.push(rows);
    }
    for table in &tables {
        if table.len() < 3 {
            panic!("SPEC.md 変換テストケース: table lacks header, separator, and data rows");
        }
        let separator = &table[1];
        let is_separator = !separator.is_empty()
            && separator
                .iter()
                .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-'));
        if !is_separator {
            panic!("SPEC.md 変換テストケース: table lacks a separator row");
        }
    }
    tables
}

fn row_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .and_then(|rest| rest.strip_suffix('|'))
        .unwrap_or_else(|| panic!("SPEC.md 変換テストケース: bad table line `{trimmed}`"));
    inner.split('|').map(str::trim).map(String::from).collect()
}

fn parse_now(cell: &str) -> (i32, u32, u32, u32, u32) {
    let shape = |detail: &str| -> ! {
        panic!("SPEC.md 変換テストケース: `now` cell must be `YYYY-MM-DD HH:MM`: {detail}: {cell}")
    };
    let b = cell.as_bytes();
    let digits = |bytes: &[u8]| bytes.iter().all(|byte| byte.is_ascii_digit());
    if b.len() != 16
        || !digits(&b[0..4])
        || !digits(&b[5..7])
        || !digits(&b[8..10])
        || !digits(&b[11..13])
        || !digits(&b[14..16])
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b' '
        || b[13] != b':'
    {
        shape("bad timestamp");
    }
    (
        cell[0..4].parse().unwrap_or_else(|_| shape("bad year")),
        cell[5..7].parse().unwrap_or_else(|_| shape("bad month")),
        cell[8..10].parse().unwrap_or_else(|_| shape("bad day")),
        cell[11..13].parse().unwrap_or_else(|_| shape("bad hour")),
        cell[14..16].parse().unwrap_or_else(|_| shape("bad minute")),
    )
}
