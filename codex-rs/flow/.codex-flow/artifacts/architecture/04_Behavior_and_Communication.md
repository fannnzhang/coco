<!-- anchor: api-design-communication -->
### 3.7. API Design & Communication

<!-- anchor: api-style -->
#### 3.7.1. API Style
The system remains CLI-driven: workflows express reasoning metadata declaratively, and the runner turns each `ResolvedStep` into either (a) a synchronous `codex exec` process launch with `--reasoning-effort` / `--reasoning-summary` flags, or (b) a mock replay call that reads stored transcripts. Structured logs and JSON artifacts serve as the public interface for downstream tooling; OpenAPI/HTTP services are not introduced to preserve the lightweight local/CI workflow.

<!-- anchor: communication-patterns -->
#### 3.7.2. Communication Patterns
- **Synchronous Request/Response:** Each step executes synchronously against `codex exec` (real mode) with stdout streaming back into the orchestrator; failures bubble immediately with the offending reasoning values for debuggability.
- **Asynchronous Logging:** Telemetry emission and mock artifact writes happen asynchronously but within the same process so they cannot outlive the step lifecycle.
- **Event Hooks for Templates:** `flow init` uses internal events to stamp reasoning-ready templates, ensuring documentation stays consistent without coupling to runtime execution.

<!-- anchor: key-interaction-flow -->
#### 3.7.3. Key Interaction Flow (Sequence Diagram)
**Description:** Shows how a reasoning override on a single step is resolved, validated, and executed.

**Diagram (PlantUML):**
~~~plantuml
@startuml
actor Author
participant "codex-flow CLI" as Flow
participant "Reasoning Resolver" as Resolver
participant "Execution Orchestrator" as Orchestrator
participant "Codex Exec Adapter" as Adapter
participant "codex exec" as CodexExec
participant "Codex API" as CodexAPI
participant "Telemetry/Mock Store" as Store

Author -> Flow : flow run --workflow agent-upgrade
Flow -> Resolver : Load workflow + overrides
Resolver --> Flow : ResolvedStep(reasoning=high)
Flow -> Orchestrator : Execute step with ResolvedStep
Orchestrator -> Adapter : Build command + pass reasoning flags
Adapter -> CodexExec : Invoke CLI (--reasoning-effort high)
CodexExec -> CodexAPI : Stream prompt
CodexAPI --> CodexExec : Result tokens
CodexExec --> Adapter : stdout/stderr + exit code
Adapter -> Store : Persist transcript + reasoning metadata
Adapter --> Orchestrator : Step result + annotations
Orchestrator --> Flow : Update logs (reasoning=high)
Flow --> Author : Display success + reasoning summary
@enduml
~~~
