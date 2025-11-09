<!-- anchor: system-context-diagram -->
### 3.3. System Context Diagram (C4 Level 1)
**Description:** Shows how the enhanced `codex-flow` runner interacts with workflow authors, automation runners, `codex exec`, and supporting repositories to ensure reasoning preferences move end-to-end.

**Diagram (PlantUML):**
~~~plantuml
@startuml
!include https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Context.puml

Person(author, "Workflow Author", "Defines workflows and tunes reasoning knobs")
Person(ci_runner, "Automation Runner", "CI or scheduler invoking flows")
System_Boundary(flow_boundary, "Flow Agent Reasoning Control") {
  System(flow_cli, "codex-flow Runner", "Parses configs, resolves reasoning, orchestrates steps")
}
System_Ext(codex_exec, "codex exec CLI", "Executes prompts against Codex models")
System_Ext(codex_api, "Codex API", "Managed LLM inference service")
System_Ext(docs_repo, "Docs & Templates Repo", "Git/markdown content surfaced by flow init")
System_Ext(mock_store, "Mock Transcript Store", "Local JSON transcripts + artifacts")

Rel(author, flow_cli, "Configures & runs workflows", "TOML / CLI")
Rel(ci_runner, flow_cli, "Triggers flows", "CI job")
Rel(flow_cli, codex_exec, "Launches with reasoning flags", "CLI / JSON")
Rel(codex_exec, codex_api, "Streams inference", "HTTPS")
Rel(flow_cli, mock_store, "Reads/Writes reasoning annotations", "File I/O")
Rel(author, docs_repo, "Pulls scaffolds", "git/flow init")
Rel(flow_cli, docs_repo, "Installs templates", "flow init")
@enduml
~~~

<!-- anchor: container-diagram -->
### 3.4. Container Diagram (C4 Level 2)
**Description:** Highlights the primary deployable units inside `codex-flow` and their interfaces to external systems.

**Diagram (PlantUML):**
~~~plantuml
@startuml
!include https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Container.puml

Person(author, "Workflow Author")
Person(ci_runner, "Automation Runner")
System_Ext(codex_exec, "codex exec CLI", "Bridge to Codex API")
System_Ext(mock_store, "Mock Transcript Store", "JSON artifacts")
System_Ext(docs_repo, "Docs & Templates Repo", "Markdown/TOML")

System_Boundary(flow_boundary, "Flow Agent Reasoning Control") {
  Container(cli_shell, "CLI Entry Point", "Rust + Clap", "Parses commands, sets runtime mode")
  Container(config_service, "Config Parser & Validator", "Rust + Serde", "Loads TOML, validates reasoning enums")
  Container(reasoning_resolver, "Reasoning Resolver", "Rust", "Merges defaults → agent → step and enforces precedence")
  Container(execution_orchestrator, "Execution Orchestrator", "Rust async", "Sequences steps, handles retries and telemetry")
  Container(adapter_codex_exec, "Codex Exec Adapter", "Rust", "Builds CLI args/JSON, streams responses")
  Container(mock_adapter, "Mock Replay & Artifact Writer", "Rust + FS", "Captures reasoning metadata for mock & real runs")
  Container(templates_docs, "Templates & Docs Scaffolder", "Rust", "Embeds reasoning examples into flow init outputs")
}

Rel(author, cli_shell, "Runs flow")
Rel(ci_runner, cli_shell, "Invokes flow via CI")
Rel(cli_shell, config_service, "Loads workflow config")
Rel(config_service, reasoning_resolver, "Provides typed configs")
Rel(reasoning_resolver, execution_orchestrator, "Hands off resolved steps")
Rel(execution_orchestrator, adapter_codex_exec, "Requests step execution")
Rel(adapter_codex_exec, codex_exec, "Spawns process with reasoning flags")
Rel(execution_orchestrator, mock_adapter, "Record reasoning level per step")
Rel(mock_adapter, mock_store, "Write/read transcripts")
Rel(templates_docs, docs_repo, "Syncs docs/templates")
Rel(author, templates_docs, "Reads scaffolds", "flow init")
@enduml
~~~

<!-- anchor: component-diagram -->
### 3.5. Component Diagram(s) (C4 Level 3)
**Description:** Details the core components inside the Execution Orchestrator container that ensure reasoning metadata propagates reliably.

**Diagram (PlantUML):**
~~~plantuml
@startuml
!include https://raw.githubusercontent.com/plantuml-stdlib/C4-PlantUML/master/C4_Component.puml

Container(flow_orchestrator, "Execution Orchestrator", "Rust", "Coordinates reasoning-aware runs") {
  Component(config_loader, "ConfigLoader", "Serde", "Reads workflow TOML into FlowConfig")
  Component(schema_validator, "SchemaValidator", "Validation layer", "Ensures enums + constraints are valid")
  Component(reasoning_engine, "ReasoningResolver", "Domain services", "Computes effective reasoning per step")
  Component(step_planner, "StepPlanner", "State machine", "Orders steps, applies overrides")
  Component(command_builder, "CommandBuilder", "Codex exec adapter", "Maps resolved settings to CLI flags/JSON")
  Component(telemetry_emitter, "TelemetryEmitter", "tracing/logger", "Annotates logs + mock artifacts")
  Component(mock_bridge, "MockBridge", "FS adapter", "Replays or records transcripts with reasoning metadata")
}
ComponentDb(config_store, "Workflow Files", "TOML")
ComponentDb(mock_store, "Mock Transcript Store", "JSON")
Component(codex_exec, "codex exec", "External CLI", "Runs prompts against Codex API")

Rel(config_loader, config_store, "Reads configs")
Rel(config_loader, schema_validator, "Validates models")
Rel(schema_validator, reasoning_engine, "Provides sanitized data")
Rel(reasoning_engine, step_planner, "Feeds resolved steps")
Rel(step_planner, command_builder, "Requests command materialization")
Rel(command_builder, codex_exec, "Spawns process")
Rel(step_planner, mock_bridge, "Requests mock or record operations")
Rel(mock_bridge, mock_store, "Read/write transcripts")
Rel(telemetry_emitter, command_builder, "Annotates events with reasoning level")
Rel(telemetry_emitter, mock_bridge, "Logs artifact locations")
@enduml
~~~

<!-- anchor: data-model-overview -->
### 3.6. Data Model Overview & ERD
**Description:** The data model centers on declarative workflow assets plus derived runtime entities that track reasoning state at each step. TOML remains the source of truth, while runtime structs capture merged settings for execution and telemetry.

**Key Entities:**
- **FlowConfig:** Top-level workflow definition referencing agents, steps, defaults.
- **AgentSpec:** Declares prompts, engines, models, and optional reasoning defaults per agent.
- **StepSpec:** References an agent and can override engine/model/prompt/reasoning plus input/output bindings.
- **ResolvedStep:** Runtime object after merging precedence, fed to the orchestrator and codex exec adapter.
- **RunTelemetry:** Captures execution outcome, reasoning level, summary mode, timestamps, and artifact paths for each step.

**Diagram (PlantUML ERD):**
~~~plantuml
@startuml
entity FlowConfig {
  *id : uuid <<PK>>
  name : string
  default_engine : string
  default_reasoning_effort : string (optional)
  default_reasoning_summary : string (optional)
}

entity AgentSpec {
  *name : string <<PK>>
  prompt_path : string
  engine : string
  model : string
  reasoning_effort : string (optional)
  reasoning_summary : string (optional)
}

entity StepSpec {
  *step_id : string <<PK>>
  agent_name : string <<FK>>
  description : string
  reasoning_effort : string (optional)
  reasoning_summary : string (optional)
  output_kind : string
  output_path : string (optional)
}

entity ResolvedStep {
  *execution_id : uuid <<PK>>
  step_id : string <<FK>>
  resolved_engine : string
  resolved_model : string
  resolved_reasoning_effort : string
  resolved_reasoning_summary : string
}

entity RunTelemetry {
  *record_id : uuid <<PK>>
  execution_id : uuid <<FK>>
  status : string
  latency_ms : integer
  log_path : string
}

FlowConfig ||--o{ AgentSpec : "defines"
FlowConfig ||--o{ StepSpec : "includes"
AgentSpec ||--o{ StepSpec : "referenced by"
StepSpec ||--|| ResolvedStep : "resolves to"
ResolvedStep ||--o{ RunTelemetry : "logs"
@enduml
~~~
