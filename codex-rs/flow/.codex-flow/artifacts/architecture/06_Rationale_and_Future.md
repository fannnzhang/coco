<!-- anchor: design-rationale -->
## 4. Design Rationale & Trade-offs

<!-- anchor: key-decisions -->
### 4.1. Key Decisions Summary
- Adopted a layered/hexagonal structure so reasoning logic is isolated in a resolver service while adapters handle codex exec, mocks, and docs independently.
- Schema-first evolution (Serde structs + validation tables) guarantees backward compatibility and fast error messaging.
- Added telemetry hooks (structured logs + mock artifacts) as first-class outputs to satisfy auditability without new storage services.
- Reused existing CLI distribution to avoid changing deployment or authentication models.

<!-- anchor: alternatives-considered -->
### 4.2. Alternatives Considered
- **Global Runtime Flag Only:** Rejected because per-step overrides (FR-002) would be impossible and workflows would lose precision.
- **Central Reasoning Service:** Considered hosting a microservice to store reasoning policies, but it would violate offline/mock requirements and add operational overhead.
- **Embedding reasoning in prompts:** Embedding directives inside prompt text would hide intent from tooling and fail to guarantee enforcement by codex exec.

<!-- anchor: known-risks -->
### 4.3. Known Risks & Mitigation
- **CLI Drift:** Upstream `codex exec` flags might change; mitigate via integration tests and compile-time constants.
- **Cost Explosion:** High reasoning by default could increase latency/cost; mitigate through sane defaults and explicit warnings when users escalate levels.
- **Unsupported Engines:** Non-Codex engines may ignore reasoning; mitigate with capability checks and warnings while still running the step.
- **Telemetry Noise:** Extra logging might overwhelm CI; mitigate by offering concise vs verbose modes while keeping structured metadata available.

<!-- anchor: future-considerations -->
## 5. Future Considerations

<!-- anchor: potential-evolution -->
### 5.1. Potential Evolution
- Adaptive reasoning policies that adjust effort based on historical latency or error rates.
- GUI/TUI editors that visualize agent/step reasoning inheritance to reduce YAML/TOML mistakes.
- Centralized policy catalogs shared across repositories for consistent enterprise defaults.
- Support for additional engines (open-source, local LLMs) through a capability registry that maps reasoning semantics.

<!-- anchor: areas-deeper-dive -->
### 5.2. Areas for Deeper Dive
- Flesh out a contract test suite between `codex-flow` and `codex exec` to detect flag regressions automatically.
- Define documentation automation so `codex-rs/docs` and `flow init` templates never diverge.
- Explore caching strategies for large mock transcripts when multiple reasoning variants are stored.

<!-- anchor: glossary -->
## 6. Glossary
- **Reasoning Effort:** Codex-supported inference depth (`minimal`, `low`, `medium`, `high`).
- **Reasoning Summary:** Toggle controlling whether Codex returns reasoning snippets (`auto`, `concise`, `detailed`, `none`).
- **AgentSpec:** Named unit in `.workflow.toml` describing prompt/model defaults for multiple steps.
- **StepSpec:** Workflow action referencing an agent and optional overrides, including reasoning fields.
- **ResolvedStep:** Runtime struct after precedence resolution, fed to codex exec.
- **Mock Transcript Store:** Filesystem location containing replay data and reasoning annotations for offline runs.
