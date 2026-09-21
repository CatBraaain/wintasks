use std::fs;

fn release_workflow() -> String {
    fs::read_to_string(format!(
        "{}/.github/workflows/release.yml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn release_workflow_builds_and_publishes_tagged_binary() {
    let workflow = release_workflow();
    for required in [
        "tags:\n      - 'v*.*.*'",
        "cargo build --release --locked",
        "target/release/wintasks.exe",
        "softprops/action-gh-release@v2",
        "generate_release_notes: true",
        "files: release/wintasks.exe",
    ] {
        assert!(
            workflow.contains(required),
            "missing workflow entry: {required}"
        );
    }
}

#[test]
fn winget_submission_waits_for_release_and_existing_package() {
    let workflow = release_workflow();
    for required in [
        "if: github.ref_name != 'v0.0.1'",
        "needs: release",
        "releases/tags/${{ github.ref_name }}",
        "winget-pkgs/tree/master/manifests/c/CatBraaain/wintasks",
        "identifier: CatBraaain.wintasks",
        "token: ${{ secrets.WINGET_TOKEN }}",
    ] {
        assert!(
            workflow.contains(required),
            "missing WinGet entry: {required}"
        );
    }
}
