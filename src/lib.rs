pub mod apply;
pub mod cron;
pub mod def;
pub mod hash;
pub mod render;
pub mod schtasks;
pub mod trigger;
pub mod xml;

use chrono::Local;

/// Local wall-clock time for this run. Passed through the conversion pipeline
/// so that tests can pin the date used for StartBoundary.
pub fn now_local() -> chrono::NaiveDateTime {
    Local::now().naive_local()
}
