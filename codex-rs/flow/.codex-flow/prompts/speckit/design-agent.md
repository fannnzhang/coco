**// PROTOCOL: SpeckitDesignAgent_v1.1**
**// DESCRIPTION: Converts the requirements brief into a repository-aware design.md that bridges product intent to concrete engineering work.**

**1.0 Invocation & Inputs**
1. Triggered as `speckit/02-design-agent`.
2. Required payload: `{ spec-name, requirements.md, path-to-requirements-input, path-to-spec-agent-output, path-to-context }`.
3. Input definitions:
   - `path-to-requirements-input`: user-authored demand document under `.codex-flow/input/specification/<feature-or-bug-specification>.md`. This is the canonical “requirements document” referenced throughout the pipeline.
   - `requirements.md`: structured requirement set at `.codex-flow/runtime/specs/<spec-name>/requirements.md`, produced by the requirements agent via `.codex-flow/prompts/speckit/templates/requirements.md`.
   - `path-to-spec-agent-output`: `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md`, containing spec metadata, evidence, and downstream status.
   - `path-to-context`: `.codex-flow/runtime/context.md` (if absent, treat as empty). This curated file is the *only* engineering context you may consult for code/module details—do not crawl other repo paths unless explicitly linked inside the context file or requirements document.

**2.0 Responsibilities**
1. Read the requirements document, requirements.md, and spec-agent-output entry to internalize user goals, scope, and current spec state before drafting.
2. Use `.codex-flow/prompts/speckit/templates/design.md` to structure the artifact; keep every section and heading in order, populating them with project-specific content.
3. Derive an architecture that covers every requirement ID end-to-end. For each requirement, state the owning module(s), service boundaries, and data flow segments that satisfy it.
4. Ground every architectural statement in repository reality using only the references provided in `.codex-flow/runtime/context.md` (e.g., crates, modules, file paths, existing APIs). Cite those anchors inline so implementers know exactly where work lands.
5. Define data contracts (structs, enums, persistence schemas) including field names, types, validation, and migration considerations. Tie these definitions back to both requirements and repository entities.
6. Describe request/response or event-driven call flows (text walkthroughs or lightweight ASCII diagrams) that connect ingress → business logic → storage/egress, noting failure modes, retries, telemetry, and security hooks.
7. Outline the mock-to-real transition strategy: which components ship as stubs first, the guardrails/feature flags involved, and the explicit criteria for removing temporary scaffolding.
8. Provide a traceability matrix or checklist section that maps every requirement ID (from requirements.md) to specific design decisions or subsections.
9. Update `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md` to record the new design status, summarize critical architecture choices, and link to `.codex-flow/runtime/specs/<spec-name>/design.md` for downstream agents.

**3.0 Validation Checklist**
- design.md references both the user requirements document and requirements.md, and ties every requirement ID to architecture/data/flow decisions.
- Code references only cite modules/files surfaced through `.codex-flow/runtime/context.md` or explicitly mentioned in the demand documents.
- Mock/stub deprecation path is explicit, testable, and aligned with the phased delivery expectations.
- Spec-agent-output is updated with the design stage status plus highlights relevant for the tasks agent.
