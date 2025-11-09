# ADR-001: Reasoning Controls Traceability

- **Status:** Proposed (Iteration I1)
- **Date:** November 8, 2025
- **Owners:** Flow Agent Reasoning Control working group
- **Stakeholders:** codex-flow maintainers, Codex CLI team, template/docs writers

## 1. Summary

codex-flow must let workflow authors declare `reasoning_effort` (`minimal|low|medium|high`) and `reasoning_summary` (`auto|concise|detailed|none`) at defaults, agent, and step scopes, resolve the effective value, and propagate it through validation, adapters, docs, and telemetry without breaking existing workflows. This ADR locks the requirement-to-code traceability so every functional requirement (FR-001–FR-005) and relevant NFR has a named implementation surface, tests, and documentation hook before any schema or runtime changes land.

## 2. Context & Drivers

- **Specification sources:** `.codex-flow/input/specification/flow-agent-reasoning-control.md` (§2.1, §6–7) and `.codex-flow/artifacts/architecture/01_Context_and_Drivers.md` enumerate the user journeys, FR/NFR list, and architectural constraints.
- **Plan alignment:** `.codemachine/artifacts/plan/01_Plan_Overview_and_Setup.md` and `02_Iteration_I1.md` define the iteration scope and list modules per task. Task `I1.T1` (this ADR) must unblock schema updates (`I1.T2`), diagrams (`I1.T3`), resolver implementation (`I1.T4`), and template/docs updates (`I1.T5`).
- **Architecture anchors:** `codex-flow-design.md` plus the architecture blueprint describe the layered runner (ConfigLoader → SchemaValidator → ReasoningResolver → adapters/telemetry) that we must keep synchronized.

## 3. Decision

1. **Traceability-first delivery:** Land this ADR under `docs/adr/` so later tasks can reference an approved scope when evolving code, diagrams, and templates.
2. **Explicit precedence rule:** Every reasoning lookup follows `workflow.defaults` → `agents.<id>` → `workflow.steps[n]`, with the first non-`None` enum winning. Resolver outputs both enums plus provenance metadata for observability/testing.
3. **Module responsibilities:**
   - `src/config/mod.rs` (`FlowConfig`, `AgentSpec`, `StepSpec`, `ResolvedStep`) owns schema evolution and serde behavior.
   - `src/config/resolver.rs` encapsulates precedence logic, returning `ResolvedStepReasoning` consumed by the runner.
   - `src/config/validation.rs` performs enum + capability validation and surfaces 100 ms error guarantees per file.
   - `src/runner/orchestrator.rs` and adapters (`adapters/codex_exec.rs`, `adapters/mock.rs`) inject resolved reasoning into real/mock executions.
   - Docs/templates (`docs/contracts/`, `docs/diagrams/`, `templates/flow-init/`, `README.md`) showcase configuration knobs and link to diagrams.
4. **Testing layers:** unit tests guard schemas/resolvers; integration tests simulate CLI propagation; `cargo insta` snapshots cover verbose logging; mock transcripts store JSON evidence in `.codex-flow/runtime/debug/`.

## 4. Functional Requirements Traceability

| ID | Requirement summary | Implementation surfaces | Tests / artifacts |
| --- | --- | --- | --- |
| FR-001 | Schema exposes reasoning enums for every agent/step | `src/config/mod.rs` updates (`FlowConfig`, `AgentSpec`, `StepSpec`, `ResolvedStep`); `src/config/defaults.rs` to supply fallback enums | `tests/config_tests.rs` ensures legacy configs deserialize; Mermaid ERD (`docs/diagrams/reasoning_data_model.mmd`) captures the new fields |
| FR-002 | Steps override agent-level reasoning | `src/config/resolver.rs` (`ReasoningResolver`, `ResolvedStepReasoning`); resolver invocation in `src/runner/orchestrator.rs` | `tests/resolver_tests.rs` cover precedence, provenance, and `None` fallbacks |
| FR-003 | CLI propagation to codex exec (real + mock) | `src/runner/orchestrator.rs` wires resolver output into adapters; `src/runner/adapters/codex_exec.rs` emits CLI flags / JSON; `src/runner/adapters/mock.rs` records reasoning in transcripts | `tests/integration/codex_exec_reasoning.rs`; `tests/integration/mock_reasoning.rs`; `.codex-flow/runtime/debug/*.json` fixtures |
| FR-004 | Validation & telemetry | `src/config/validation.rs` enforces enums + engine capability matrix; `src/runner/telemetry.rs` logs `reasoning=<level>` when `--verbose` is enabled | `tests/resolver_tests.rs` invalid-value cases; telemetry snapshots `tests/snapshots/runner_verbose.snap` |
| FR-005 | Templates/docs demonstrate usage | `templates/flow-init/default.workflow.toml`, `.codex-flow/workflows/*.workflow.toml` examples, `README.md`, `docs/contracts/codex_exec_reasoning.md`, diagram cross-reference in this ADR | `flow init` smoke test (manual/automation); doc linting if configured |

## 5. Non-Functional Requirement Alignment

| NFR ID | Concern | Coverage approach |
| --- | --- | --- |
| NFR-COMP-001 | Backwards-compatible deserialization | Keep new fields `Option<ReasoningEffort>` / `Option<ReasoningSummary>` with serde `default`; fixture tests prove absence handling |
| NFR-UX-001 | Observability in verbose output | Extend `RunnerEvent` formatting + telemetry snapshots to append `reasoning=<level>, summary=<mode>` without impacting default logs |
| NFR-CONF-001 | Validation latency | Run enum/capability validation during config load only once per file; reuse existing diagnostics framework to stay < 100 ms/file |
| NFR-DOC-001 | Documentation freshness | Co-deploy ADR, contract doc, README snippet, and template updates during I1/I2 so docs never lag code |

## 6. Precedence & Validation Model

- **Lookup algorithm:**
  1. Start with `FlowConfig.defaults.reasoning_*` (if defined).
  2. Merge in `AgentSpec.reasoning_*` for the referenced agent.
  3. Apply `StepSpec.reasoning_*` overrides.
  4. Emit a `ResolvedStepReasoning { effort, summary, source }` struct storing both enums plus the source tier (`Default`, `Agent`, `Step`).
- **Validation:**
  - Enum parsing is centralized in `ReasoningEffort` / `ReasoningSummary` newtypes with `FromStr` + serde `rename_all = "snake_case"` to guarantee consistent TOML casing.
  - `validation.rs` checks whether the chosen engine (`codex`, `codemachine`, future engines) advertises reasoning support. Unsupported combinations raise `ConfigError::UnsupportedReasoning { engine, field }` with actionable messages.
  - Steps cannot specify reasoning if their referenced agent belongs to an engine that forbids overrides; validator surfaces that earlier than runtime.
- **Error messaging:** leverage the existing diagnostic formatter so CLI users see “valid values: minimal|low|medium|high” style hints, satisfying FR-004 + NFR-CONF-001.

## 7. Module & Artifact Impact Map

- `src/config/mod.rs`: add enums, serde defaults, and ensure TOML docs reference new keys.
- `src/config/defaults.rs`: expose helper methods so tests can construct configs with reasoning defaults quickly.
- `src/config/resolver.rs`: introduce `ReasoningResolver` plus helper APIs consumed by orchestrator/tests.
- `src/config/validation.rs`: extend capability matrix + error variants.
- `src/runner/orchestrator.rs`: augment step execution context with `ResolvedStepReasoning` and pass it downstream.
- `src/runner/adapters/codex_exec.rs`: add CLI `--reasoning-effort` / `--reasoning-summary` flags or JSON payload fragments and unit tests for command building.
- `src/runner/adapters/mock.rs`: store reasoning metadata in `.codex-flow/runtime/debug/*.json`.
- `src/runner/telemetry.rs`: append reasoning info to verbose log lines.
- `docs/diagrams/reasoning_data_model.mmd` & `docs/diagrams/reasoning_component.puml`: visualize schema + component interactions (I1.T2/I1.T3 deliverables) and keep this ADR linked to the component diagram.
- `docs/contracts/codex_exec_reasoning.md`, `README.md`, `templates/flow-init/default.workflow.toml`: user-facing education for FR-005.

## 8. Verification & Testing Strategy

1. **Unit tests:**
   - `tests/config_tests.rs` for serde/back-compat coverage (loading configs with/without new fields).
   - `tests/resolver_tests.rs` for precedence ladders, invalid enums, provenance metadata, and compatibility fallbacks.
2. **Integration tests:**
   - `tests/integration/codex_exec_reasoning.rs` ensures adapters emit CLI flags/JSON expected by Codex exec and covers agent default vs. step override vs. defaults-only flows.
   - `tests/integration/mock_reasoning.rs` validates mock transcripts capture reasoning metadata for audit/debug flows.
3. **Snapshots:** `tests/snapshots/runner_verbose.snap` (via `cargo insta`) records verbose telemetry lines with reasoning annotations.
4. **Manual/CLI checks:** `flow init` scaffolding review plus PlantUML/Mermaid renders (`make diagrams` or `./scripts/render_diagrams.sh`) for docs.

## 9. Assumptions, Risks, and Follow-Ups

- **Assumptions:** Codex CLI keeps `--reasoning-effort` / `--reasoning-summary` stable; workspace already ships `plantuml`/`mermaid-cli` helpers; mock transcripts remain JSON-compatible.
- **Risks:**
  - Drift between resolver output and adapter expectations → mitigate by sharing the `ResolvedStepReasoning` struct and reusing enums across modules.
  - Template/docs lag code → mitigated by gating downstream tasks on this ADR reference and codifying updates in Iterations I1/I4.
  - Validation latency regressions → mitigate via targeted benchmarks during `just fix -p codex-flow` runs before merging.
- **Follow-ups:**
  - I1.T2 references this ADR when updating schemas and ERD.
  - I1.T3 adds component diagram and updates the “Related Artifacts” section below with the file path.
  - Later iterations append telemetry/logging evidence back to this ADR if behavior deviates from the baseline decision.

## 10. Related Artifacts & References

- `.codex-flow/input/specification/flow-agent-reasoning-control.md`
- `.codex-flow/artifacts/architecture/01_Context_and_Drivers.md`
- `.codemachine/artifacts/plan/01_Plan_Overview_and_Setup.md`
- `.codemachine/artifacts/plan/02_Iteration_I1.md`
- `codex-flow-design.md`
- Pending diagrams: `docs/diagrams/reasoning_data_model.mmd`, `docs/diagrams/reasoning_component.puml` (to be produced in I1)

