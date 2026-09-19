//! CLI entry point: collects arguments and delegates to the library.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let code = wintasks::run(&args, &mut stdout, &mut stderr);
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use wintasks::{parse_apply, parse_render, run, ApplyCli, RenderCli};

    /// Runs the CLI with byte-vector sinks so stdout and stderr are
    /// observable as strings.
    fn run_cli(args: &[&str]) -> (u8, String, String) {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&owned, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    fn temp_path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("wintasks-cli-test-{}-{name}", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn no_arguments_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&[]);
        assert_eq!(code, 2);
        assert!(err.contains("usage: wintasks"), "{err}");
    }

    #[test]
    fn unknown_command_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&["bogus"]);
        assert_eq!(code, 2);
        assert!(err.contains("unknown command `bogus`"), "{err}");
    }

    #[test]
    fn unknown_argument_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&["render", "--bogus"]);
        assert_eq!(code, 2);
        assert!(err.contains("unexpected argument `--bogus`"), "{err}");
    }

    #[test]
    fn option_without_value_prints_usage_and_exits_nonzero() {
        let (code, _, err) = run_cli(&["apply", "--path"]);
        assert_eq!(code, 2);
        assert!(err.contains("--path requires a value"), "{err}");
    }

    #[test]
    fn render_rejects_apply_only_options() {
        for args in [
            vec!["render", "--mount", "M"],
            vec!["render", "--dry-run"],
            vec!["render", "--prune"],
        ] {
            let (code, _, err) = run_cli(&args);
            assert_eq!(code, 2, "{args:?}");
            assert!(err.contains("only valid for apply"), "{err}");
        }
    }

    #[test]
    fn options_default_to_spec_values() {
        let RenderCli { path } = parse_render(&[]).unwrap();
        assert_eq!(path, "wintasks.yaml");
        let ApplyCli { path, opts } = parse_apply(&[]).unwrap();
        assert_eq!(path, "wintasks.yaml");
        assert_eq!(opts.mount, "WinTasks");
        assert!(!opts.dry_run);
        assert!(!opts.prune);
    }

    #[test]
    fn apply_accepts_apply_only_options() {
        let args = ["--mount", "M", "--dry-run", "--prune"].map(String::from);
        let ApplyCli { opts, .. } = parse_apply(&args).unwrap();
        assert_eq!(opts.mount, "M");
        assert!(opts.dry_run);
        assert!(opts.prune);
    }

    #[test]
    fn render_reads_yaml_from_explicit_path() {
        let path = temp_path("render-explicit.yaml");
        std::fs::write(
            &path,
            "- name: Hello\n  trigger: { type: now, value: v }\n  action: { command: cmd.exe }\n",
        )
        .unwrap();
        let (code, out, _) = run_cli(&["render", "--path", &path]);
        std::fs::remove_file(&path).ok();
        assert_eq!(code, 0);
        assert!(out.contains("--- Hello ---"), "{out}");
    }

    #[test]
    fn rejects_directory_as_path() {
        // Directories are not valid --path values (spec); read_to_string
        // fails with EISDIR.
        let dir = temp_path("not-a-file-dir");
        std::fs::create_dir(&dir).unwrap();
        let (code, _, err) = run_cli(&["render", "--path", &dir]);
        std::fs::remove_dir(&dir).ok();
        assert_eq!(code, 1);
        assert!(err.contains(&dir), "{err}");
    }
}
