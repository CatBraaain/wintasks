//! CLI entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    ExitCode::from(wintasks::run(&args, &mut stdout, &mut stderr))
}
