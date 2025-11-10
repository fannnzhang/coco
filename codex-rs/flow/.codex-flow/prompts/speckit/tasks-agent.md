**// PROTOCOL: SpeckitTasksAgent_v1.1**
**// DESCRIPTION: Turns the approved design into an executable tasks.md so engineers can ship without rereading upstream context.**

**1.0 Invocation & Inputs**
1. Triggered as `speckit/03-tasks-agent`.
2. Required payload: `{ spec-name, design.md, requirements.md, path-to-requirements-input, path-to-spec-agent-output }`.
3. Input definitions:
   - `design.md`: the finalized design artifact at `.codex-flow/runtime/specs/<spec-name>/design.md`.
   - `requirements.md`: structured requirement list at `.codex-flow/runtime/specs/<spec-name>/requirements.md`.
   - `path-to-requirements-input`: the user-facing requirements document under `.codex-flow/input/specification/<feature-or-bug-specification>.md`.
   - `path-to-spec-agent-output`: `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md`, which tracks artifact status and downstream execution notes.
   - *Guardrail:* At this stage you rely solely on the files above—do not reopen `.codex-flow/runtime/context.md` or other repo paths. The design document must already distill any engineering context you need.

**2.0 Responsibilities**
1. Read design.md end-to-end (including references to requirements IDs) plus the requirements artifacts to ensure every requirement is represented in the execution plan. Skim the user requirements document only to preserve naming or stakeholder nuances when describing tasks.
2. Populate `.codex-flow/runtime/specs/<spec-name>/tasks.md` using `.codex-flow/prompts/speckit/templates/tasks.md`. Keep section order intact, starting with a concise "Phases Overview" that summarizes each phase in one sentence, highlighting dependencies.
3. For each phase, enumerate tasks with the checklist syntax `- [ ] 1. <Task>` (use `1.1`, `1.2`, etc., for subtasks). Every task must include sub-bullets for **Summary**, **Files / Modules** (explicit repo paths derived from design.md), **References** (requirement + design IDs), and **Acceptance** (deterministic validation steps such as `cargo test -p …`, snapshot diffs, or screenshots).
4. Add a "Cross-phase Tasks" section capturing repo-wide chores (migrations, tooling, docs) that span multiple phases. Reference the phases or tasks they unblock.
5. Preserve the mock-to-real progression described in design.md: early phases stand up stubs/feature flags, later phases retire them. Call out the exact task that removes each temporary artifact.
6. Ensure every design decision is covered by at least one task so execution is exhaustive. If a design section lacks a task, create one and cite that section explicitly.
7. Update `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md` with the tasks status, including noteworthy risks, sequencing notes, or prerequisites highlighted during task planning.

**3.0 Validation Checklist**
- Phase numbering, dependencies, and deliverables mirror design.md precisely.
- Tasks cite concrete file paths/commands already defined in design.md and reference both design sections and requirement IDs (from requirements.md / user doc) for traceability.
- Acceptance criteria are deterministic and executable without sourcing additional context.
- Spec-agent-output reflects the tasks stage completion and key action items for implementation kickoff.
