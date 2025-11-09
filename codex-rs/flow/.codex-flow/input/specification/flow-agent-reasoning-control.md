### Part 1: The Essentials

#### 1.0 Project Overview
- **1.1 Project Name:** Flow Agent Reasoning Control
- **1.2 Project Goal:** Extend `codex-flow` so every agent and workflow step can declare its desired model reasoning effort and summary preference, and have those settings propagated to `codex exec`, eliminating the current hard-coded defaults.
- **1.3 Target Audience:** Flow workflow authors, Codex CLI maintainers, and internal automation teams orchestrating multi-agent pipelines.

#### 2.0 Core Functionality & User Journeys

- **2.1 Core Features List:**
  - `codex-flow` **MUST** add `reasoning_effort` and `reasoning_summary` fields to agent definitions and per-step overrides, matching the enums exposed by Codex (`minimal|low|medium|high` and `auto|concise|detailed|none`).
  - The Flow runner **MUST** pass the resolved reasoning settings to `codex exec` (mock + real modes) so that downstream Codex conversations honor the requested inference depth.
  - Configuration validation **SHOULD** catch unsupported reasoning values or attempts to set reasoning on engines that do not expose the feature.
  - `flow init` scaffolding and prompt/workflow templates **SHOULD** showcase how to set reasoning levels per agent to drive adoption.
  - Runtime logs, mock replays, and collected artifacts **MAY** annotate the reasoning level that was in effect to help debug unexpected latency or output verbosity.

- **2.2 User Journeys:**
  1. `User edits .codex-flow/workflows/agent-reasoning-upgrade.workflow.toml to set reasoning_effort = "high" for the code-generation agent` → app **MUST** load the new fields without warnings → `Run codex-flow run --workflow agent-reasoning-upgrade` → `codex exec` **MUST** launch that agent with high reasoning, and CLI output confirms the override.
  2. `User keeps the agent default at medium but overrides a single step with reasoning_effort = "minimal"` → app **MUST** prefer the step override when invoking `codex exec` for that step → outcome: targeted steps finish faster without altering the rest of the workflow.
  3. `User sets reasoning_effort = "turbo" (invalid)` → app **MUST** fail fast during config parsing, highlighting the valid enum values, → outcome: user fixes typo before any agents start.
  4. `User runs in mock mode with reasoning overrides` → app **SHOULD** record the requested reasoning level inside `.codex-flow/runtime/debug/*.json` even though no real Codex call occurs → outcome: dry runs accurately preview runtime knobs.

#### 3.0 Data Models
- **AgentSpec:** `name` (REQUIRED, string, unique key), `prompt` (REQUIRED path, must exist), `engine` (OPTIONAL, defaults to `codex`), `model` (OPTIONAL, defaults to workspace model), `reasoning_effort` (OPTIONAL enum minimal|low|medium|high), `reasoning_summary` (OPTIONAL enum auto|concise|detailed|none), `description` (OPTIONAL string ≤ 280 chars).
- **StepSpec:** `agent` (REQUIRED, references AgentSpec), `description` (OPTIONAL string), `engine`/`model`/`prompt` overrides (OPTIONAL), `reasoning_effort` (OPTIONAL enum, overrides agent), `reasoning_summary` (OPTIONAL enum), `input.template` (OPTIONAL path/string), `output.kind` (REQUIRED when present, `stdout|file`), `output.path` (REQUIRED when `kind = file`).
- **ResolvedStep:** `engine` (REQUIRED string), `model` (REQUIRED string), `prompt_path` (REQUIRED string), `reasoning_effort` (OPTIONAL enum), `reasoning_summary` (OPTIONAL enum); values result from merging defaults → agent → step overrides.

#### 4.0 Essential Error Handling
- **Invalid Reasoning Value:** app **MUST** surface a descriptive error and refuse to run the workflow.
- **Unsupported Engine:** if a non-Codex engine is selected and does not accept reasoning flags, app **SHOULD** warn and ignore reasoning rather than crash.
- **Missing Prompt:** app **MUST** keep existing prompt existence checks; reasoning changes must not mask filesystem errors.
- **CLI Failure:** if `codex exec` rejects the reasoning combination, Flow **SHOULD** bubble up stderr plus the offending settings to help diagnose issues.
- **No Internet / API outage:** Flow **SHOULD** behave as today (mock mode, retries) while still showing which reasoning level had been requested.

---

### Part 2: Advanced Specifications

#### 5.0 Formal Project Controls & Scope
- **5.1 Document Control:** Version 0.1 | Status Draft | Date 2025-11-08
- **5.2 Detailed Scope:**
  - **In Scope:** Flow config schema updates, serde + validation logic, CLI flag plumbing, mock engine annotations, docs + templates + examples, targeted tests for config parsing and engine invocation.
  - **Out of Scope:** New reasoning algorithms, Codex backend changes, UI for editing reasoning inside TUI, scheduler-level multi-agent branching.
- **5.3 Glossary:**
  | Term | Definition |
  | --- | --- |
  | Reasoning Effort | Discrete inference depth levels supported by Codex models (`minimal/low/medium/high`). |
  | Reasoning Summary | Codex toggle that controls whether summary snippets of reasoning are returned. |
  | Flow Agent | A configured prompt + model pair inside `.codex-flow/workflows/*.workflow.toml`. |
  | Mock Mode | Flow execution mode that replays stored transcripts instead of hitting Codex. |

#### 6.0 Granular & Traceable Requirements
| ID | Requirement Name / User Story | Description | Priority |
| --- | --- | --- | --- |
| FR-001 | Agent Schema Supports Reasoning | `FlowConfig.agents.*` **MUST** accept `reasoning_effort` + `reasoning_summary` and expose them throughout the runner. | Critical |
| FR-002 | Step Overrides | Individual workflow steps **MUST** be able to override an agent’s reasoning settings without mutating the source agent. | High |
| FR-003 | CLI Propagation | When invoking `codex exec`, Flow **MUST** pass the resolved reasoning values via CLI flags or JSON payload to ensure Codex honors them. | Critical |
| FR-004 | Validation & Telemetry | Flow **SHOULD** validate values on load and log the active reasoning level per step (mock + real) for auditability. | Medium |
| FR-005 | Templates & Docs | `flow init` scaffolding, README snippets, and example workflows **SHOULD** showcase per-agent reasoning fields to accelerate adoption. | Medium |

#### 7.0 Measurable Non-Functional Requirements
| ID | Category | Requirement | Metric / Acceptance Criteria |
| --- | --- | --- | --- |
| NFR-COMP-001 | Compatibility | Existing configs without reasoning fields **MUST** continue to load unchanged. | 0 breaking deserialization errors in regression tests. |
| NFR-UX-001 | Observability | CLI verbose output **SHOULD** display the active reasoning level for each step. | `codex-flow run --verbose` shows `reasoning=high` (or equivalent) per step. |
| NFR-CONF-001 | Config Validation | Invalid reasoning values **MUST** be rejected in < 100ms per file to keep `flow run` responsive. | Benchmarked on representative configs. |
| NFR-DOC-001 | Documentation | Updated docs **MUST** land alongside code changes. | README + templates mention reasoning fields. |

#### 8.0 Technical & Architectural Constraints
- **8.1 Technology Stack:** Rust (`codex-flow` crate), Serde for config parsing, existing `codex exec` CLI (JSON streaming), TOML workflow definitions.
- **8.2 Architectural Principles:** Maintain backwards compatibility, prefer composable defaults/overrides flow (defaults → agent → step), keep reasoning plumbing engine-agnostic to enable future engines.
- **8.3 Deployment Environment:** No change—`codex-flow` binaries continue shipping via existing release process; feature gated only by config.

#### 9.0 Assumptions, Dependencies & Risks
- **Assumptions:** Codex CLI already accepts reasoning parameters; Flow runners have access to same binaries; workflow authors are comfortable editing TOML by hand.
- **Dependencies:** Upstream Codex models must support requested reasoning levels; documentation pipeline must be updated in `codex-rs/docs` to explain the new keys.
- **Risks:** Misconfigured reasoning could increase token latency/cost; need guardrails + defaults. Incomplete validation could emit silent no-ops if CLI flag names drift—must add integration tests.
