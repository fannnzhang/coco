<!-- anchor: cross-cutting-concerns -->
### 3.8. Cross-Cutting Concerns

<!-- anchor: authentication-authorization -->
#### 3.8.1. Authentication & Authorization
- Relies on existing Codex CLI credentials (API keys, seatbelt sandbox indicators). Runner ensures sensitive values are never persisted in mock artifacts; only reasoning metadata and references to credential scopes appear in logs.
- Local filesystem permissions gate who can edit workflows or read transcripts; CI environments reuse the same service accounts.

<!-- anchor: logging-monitoring -->
#### 3.8.2. Logging & Monitoring
- Use `tracing` spans per step with structured fields (`step_id`, `agent`, `reasoning_effort`, `summary_mode`, `engine`).
- Verbose mode prints reasoning info inline for humans, while JSON logs feed into CI artifacts for auditability.
- Mock store embeds reasoning metadata alongside responses so dry runs mirror real execution visibility.

<!-- anchor: security-considerations -->
#### 3.8.3. Security Considerations
- Validate inputs early to prevent command injection via reasoning parameters (only allow enumerated strings).
- Continue enforcing prompt path existence to avoid silent fallbacks that could leak information.
- Keep secrets out of telemetry by redacting environment variables before logging invocation contexts.

<!-- anchor: scalability-performance -->
#### 3.8.4. Scalability & Performance
- Services are stateless per run; multiple workflows can execute concurrently via separate CLI invocations.
- Reasoning resolver caches merged agent defaults to minimize repeated merges when steps share agents.
- Validation completes within the NFR target (<100 ms) by leveraging Serde + small lookup tables for enums.
- For high-volume CI pipelines, logs can be throttled (e.g., summary-only) while still noting reasoning levels.

<!-- anchor: reliability-availability -->
#### 3.8.5. Reliability & Availability
- Mock mode remains the fallback for outages; reasoning metadata is still recorded so users know what would have run.
- CLI propagates any `codex exec` failure along with the exact reasoning settings, simplifying retries.
- Add regression tests that pin CLI flag names to catch upstream drift early.

<!-- anchor: deployment-view -->
### 3.9. Deployment View

<!-- anchor: target-environment -->
#### 3.9.1. Target Environment
Runs on developer laptops, CI agents, and automation servers where `codex-flow` binaries already live. Externally, it depends on Codex API endpoints reachable over HTTPS and versioned documentation repositories.

<!-- anchor: deployment-strategy -->
#### 3.9.2. Deployment Strategy
- Distribute updates through the existing release pipeline (crates, Homebrew, internal package feeds).
- No new infrastructure; feature toggled purely through workflow configuration. Docs and templates update via repo merges.
- Snapshot tests (`cargo insta`) ensure UI/log regressions are captured before release.

<!-- anchor: deployment-diagram -->
#### 3.9.3. Deployment Diagram (PlantUML)
~~~plantuml
@startuml
!include https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Deployment.puml

deploymentNode(dev_machine, "Developer Laptop", "macOS/Linux") {
  deploymentNode(flow_workspace, ".codex-flow Workspace", "Filesystem") {
    artifact(configs, "Workflow Configs", "TOML")
    artifact(transcripts, "Mock Transcripts", "JSON")
  }
  node(flow_cli, "codex-flow Binary", "Rust CLI")
}

deploymentNode(ci_agent, "CI Runner", "Ubuntu container") {
  node(ci_flow_cli, "codex-flow (CI)", "Rust CLI")
}

deploymentNode(codex_cloud, "Codex Cloud", "Managed LLM") {
  node(codex_api, "Codex API", "HTTPS Service")
}

Rel(flow_cli, configs, "reads")
Rel(flow_cli, transcripts, "reads/writes reasoning metadata")
Rel(flow_cli, codex_api, "Invokes via codex exec")
Rel(ci_flow_cli, codex_api, "Invokes via codex exec")
Rel(ci_flow_cli, configs, "pulls via git")
Rel(ci_flow_cli, transcripts, "stores artifacts")
@enduml
~~~
