use std::fs;

use codex_flow::config::FlowConfig; // will fail unless lib; adjust to CLI-style test

#[test]
fn parse_minimal_config() {
    // Create a minimal config in temp dir
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("flow.toml");
    fs::write(
        &cfg_path,
        r#"
[defaults]
engine = "codex"
mock = true

[agents.a]
prompt = "prompts/templates/codemachine/workflows/git-commit-workflow.md"

[workflows.wf]
  [[workflows.wf.steps]]
  agent = "a"
  model = "gpt-5"
"#,
    )
    .unwrap();

    let cfg = FlowConfig::load(&cfg_path).unwrap();
    assert!(cfg.workflows.contains_key("wf"));
}
