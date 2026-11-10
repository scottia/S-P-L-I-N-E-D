use std::fs;

#[test]
fn macos_development_workflow_is_branch_scoped_and_non_publishing() {
    let workflow = fs::read_to_string(".github/workflows/macos-development.yml")
        .expect("macOS development workflow");

    assert!(workflow.contains("runs-on: macos-latest"));
    assert!(workflow.contains("branches: [dev-macos]"));
    assert!(workflow.contains("cargo +stable test --locked"));
    assert!(workflow.contains("cargo +stable build --locked --release"));
    assert!(workflow.contains("splined-macos-development.tar.gz"));
    assert!(workflow.contains("retention-days: 14"));

    for forbidden in [
        "gh release",
        "git tag",
        "git push",
        "docker push",
        "msstore",
        "contents: write",
    ] {
        assert!(
            !workflow.to_ascii_lowercase().contains(forbidden),
            "development workflow contains publishing operation: {forbidden}"
        );
    }
}
