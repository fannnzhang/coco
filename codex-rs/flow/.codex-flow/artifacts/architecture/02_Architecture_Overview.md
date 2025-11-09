<!-- anchor: proposed-architecture -->
## 3. Proposed Architecture

<!-- anchor: architectural-style -->
### 3.1. Architectural Style
A layered, schema-first CLI architecture with hexagonal (ports-and-adapters) traits keeps reasoning concerns cohesive: the **Domain layer** (Flow config + reasoning resolver) owns validation and override precedence; the **Application layer** (workflow runner) sequences steps and coordinates telemetry; **Adapters** (Codex exec bridge, mock replay, docs/templates) translate domain intents to external tools. This style isolates reasoning logic from transport specifics, simplifies testing via mock adapters, and satisfies the requirement to reuse the same binaries across local and CI runners.

<!-- anchor: technology-stack-summary -->
### 3.2. Technology Stack Summary
| Area | Selection | Justification |
| --- | --- | --- |
| Frontend / UX | CLI + markdown docs | Requirements target workflow authors operating via CLI and reading docs/templates; no new GUI needed. |
| Backend Language / Framework | Rust (`codex-flow` crate) with async runtimes already in use | Reuses existing codebase, benefits from strong typing for config schemas and fast validation (<100 ms). |
| Configuration & Data Stores | Local filesystem (`.codex-flow/workflows/*.toml`, JSON transcripts, Markdown docs) | Aligns with current delivery model; enables offline mock runs and artifact inspection. |
| Messaging / Queues | In-process channels only | Step orchestration remains deterministic and synchronous; no external broker required. |
| External Services | `codex exec` CLI → Codex API (HTTPS) | Only path that can actually execute reasoning-aware prompts; CLI already trusted within workflows. |
| Containerization / Orchestration | Not required (pre-built binaries) | Feature ships as part of existing releases; no runtime containers added. |
| Key Libraries / Tools | Serde (config), Clap (CLI), `tracing` (logs), `cargo-insta` (snapshot tests), `just` tasks | Provide schema evolution, verbose output, and regression coverage demanded by FR-004/NFRs. |
