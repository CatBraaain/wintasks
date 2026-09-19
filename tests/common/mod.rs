//! Shared helpers for integration tests.
//!
//! Each integration test file pulls this in with `mod common;`.

/// Pinned wall-clock time (2026-01-15T10:00:00) so expected StartBoundary
/// and dry-run preview values stay stable across tests. Unused in test
/// binaries that do not pin the clock.
#[allow(dead_code)]
pub const FIXED_NOW: chrono::NaiveDateTime = match chrono::NaiveDate::from_ymd_opt(2026, 1, 15) {
    Some(date) => match date.and_hms_opt(10, 0, 0) {
        Some(datetime) => datetime,
        None => panic!("FIXED_NOW: invalid time"),
    },
    None => panic!("FIXED_NOW: invalid date"),
};

/// Writes `contents` to a unique temp YAML file and returns its path.
/// The caller removes the file when done.
pub fn write_temp_yaml(test_name: &str, contents: &str) -> String {
    let mut file = tempfile::Builder::new()
        .prefix(&format!("wintasks-it-{test_name}-"))
        .suffix(".yaml")
        .tempfile()
        .expect("create temp YAML file");
    std::io::Write::write_all(&mut file, contents.as_bytes())
        .expect("write temp YAML file");
    let (_dir, path) = file.keep().expect("keep temp YAML file");
    path.to_string_lossy().into_owned()
}

/// Runs the CLI `render --path <path>` and returns (exit code, stdout, stderr).
/// Unused in test binaries that call `run` directly.
#[allow(dead_code)]
pub fn run_render(path: &str) -> (u8, String, String) {
    let args = ["render", "--path", path].map(String::from);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = wintasks::run(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}
