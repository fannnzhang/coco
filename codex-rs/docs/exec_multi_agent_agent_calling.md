# Multi-Agent Invocation Support in `codex exec`

## Goals
- Allow the primary model to launch additional Codex agents via a structured tool call (human-friendly form: `agent <agent-id> "<prompt>"`).
- Preserve the existing shell execution semantics for `exec`, while reusing tooling infrastructure (approvals, sandboxing, orchestration).
- Stream all intermediate output from the sub-agent to the human-facing CLI without feeding it back into the primary model.
- Return only the sub-agent’s final response (`last_agent_message`) to the caller as the tool result to keep the conversation context clean.
- Support both human (`stderr`) and JSONL (`stdout`) output modes.

## High-Level Architecture
1. **Configuration**  
   - Extend `codex_core::config::Config` to expose an `agents` registry (`HashMap<String, AgentConfig>`).  
   - Each `AgentConfig` specifies model overrides, optional profile, prompt source, and metadata.  
   - Update CLI/config loaders (`config.md`, CLI flags) to surface the registry.

2. **Tool Entry Point**  
   - Teach `ShellHandler::run_exec_like` (`core/src/tools/handlers/shell.rs`) to intercept `["agent", "<id>", "<prompt>"]`.  
   - Parse into `AgentInvocationParams` and delegate to a new `AgentRunner` instead of dropping through to shell execution.  
   - Reject unknown agents or malformed arguments with `FunctionCallError::RespondToModel`.

3. **AgentRunner**  
   - Lives in a new module (e.g., `core/src/tools/handlers/agent.rs`).  
   - Responsibilities: resolve profile, spawn sub-conversation, stream events, capture final output, translate to `ToolOutput`.

## Agent Configuration
- Registry: `Config.agents: HashMap<String, AgentConfig>` populated from user config files. Key must match the `name` field inside each entry.
- Struct shape:
  - `name: String` — required identifier (e.g., `commit-agent`); referenced in the `agent <name> "prompt"` call.
  - `prompt_path: PathBuf` — required path to the static prompt file that seeds the agent turn.
  - `model: Option<String>` — optional explicit model override.
  - `profile: Option<String>` — optional profile name; takes precedence over `model` by reusing exec’s existing profile machinery.
  - `reasoning_effort: Option<ModelReasoningEffort>` / `reasoning_summary: Option<ModelReasoningSummary>` — optional overrides for reasoning settings. Omitted fields inherit the caller’s turn configuration.
  - `description: Option<String>` — human-readable summary for help/logging.
- Validation during config load ensures prompt files exist and `name` matches the map key.
- CLI may expose additional flags (future work) to register/override agents; baseline implementation relies on config files only.

## Execution Flow
1. **Session Bootstrap**  
   - Use `ConversationManager::new_conversation` to start a fresh `CodexConversation` configured from the chosen `AgentConfig`.  
   - Build `Op::UserTurn` with the agent prompt (and optional images, schema) mirroring `exec/src/lib.rs:332`.

2. **Event Loop**  
   - Instantiate a dedicated `EventProcessorWithHumanOutput` (ANSI config inherits from parent turn).  
   - Prefix every emitted line with the agent id to avoid ambiguity (`[agent:<id>] ...`).  
   - If the parent CLI runs with `--json`, also run `EventProcessorWithJsonOutput` and buffer the produced JSON lines for structured listeners.

3. **Completion**  
   - Stop when the sub-agent emits `EventMsg::TaskComplete`; capture `last_agent_message`.  
   - On `EventMsg::Error`/`TurnFailed`, propagate failure to the main model (`success: Some(false)`).
   - Return `ToolOutput::Function { content: final_message, success: Some(true) }`.

## Output Routing
- **Human Mode**  
  - Sub-agent intermediate output goes straight to the CLI via the dedicated human processor; no rewriting of parent session history.  
  - Emit start/finish markers through `ts_msg!` to frame the sub-session (e.g., “⬡ agent commit-agent started”).
- **JSON Mode**  
  - Wrap buffered JSON lines into `ThreadEvent::AgentSession { agent_id, events }` (new enum entry) before printing, preserving a single JSONL line per aggregated event.
- **Primary Model Context**  
  - Only the final message is sent back in the tool result. No intermediate deltas or logs reach the primary model.

## Abort & Shutdown Semantics
- If the parent turn is cancelled (`Op::Interrupt`), immediately forward `Op::Shutdown` to the sub-agent and wait for `ShutdownComplete`.
- Support optional `--timeout` argument for agent invocations; apply via `tokio::time::timeout`.
- Ensure telemetry/tracing spans are tagged with `agent_id` for observability.

## Testing Strategy
1. **Unit Tests**  
   - Command parsing (`["agent", ...]`) → `AgentInvocationParams`.  
   - Config loading of `AgentConfig`.  
   - Error cases (unknown agent, empty prompt).
2. **Integration Tests**  
   - New `exec/tests/sub_agent.rs`: simulate tool call, assert human stderr shows agent output while tool result equals final message.  
   - JSON mode variant verifying `ThreadEvent::AgentSession`.
3. **Cancellation/Timeout**  
   - Test parent interrupt propagates shutdown.  
   - Test timeout surfaces as failure with explanatory message.

## Incremental Rollout
- Phase 1: support single nested agent at a time, no session reuse.  
- Phase 2 (optional): reuse cached agent sessions or add agent-to-agent calls.  
- Document configuration and usage in `docs/exec_multi_agent_agent_calling.md` and user-facing help.
