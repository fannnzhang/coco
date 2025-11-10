**// PROTOCOL: SpeckitRequirementsAgent_v1.0**
**// DESCRIPTION: Generates the requirements.md artifact from a fully populated requirements input brief.**

**1.0 Invocation & Inputs**
1. Triggered as `speckit/01-requirements-agent`.
2. Required payload: `{ spec-name, path-to-requirements-input, path-to-spec-agent-output }`.
3. Inputs include:
   - `.codex-flow/input/specification/<feature-or-bug-specification>.md` (the path supplied via `path-to-requirements-input`), which is the canonical user requirements document for this spec.
   - `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md`, which declares the target spec, project background, and context needed for downstream agents.
   - Any evidence links referenced by those documents.

**2.0 Responsibilities**
1. Read the requirements-input brief, the referenced evidence, and `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md` to understand the project background, user profile, and the exact spec scope before writing.
2. Produce `specs/<spec-name>/requirements.md` using `templates/requirements.md`. Preserve every template section (Introduction, Requirements Summary, etc.) in the provided order and keep headings intact.
3. Break the user demand into discrete, stable-ID requirements—one per user need or system responsibility—ensuring full coverage of the signals from the brief and the spec context.
4. Write every requirement statement in strict EARS form (`WHEN … THEN the system SHALL … SO THAT …`) and attach at least two acceptance criteria that validate the requirement end to end.
5. Capture non-functional and cross-cutting requirements that apply globally to the spec, including dependencies on other systems or teams.
6. Cite the evidence (brief excerpts, upstream agent notes, spec references) that justifies each requirement so downstream agents can trace reasoning.
7. Update `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md` to reflect the completed requirements work: point to the generated `requirements.md`, summarize notable requirements, and ensure the spec metadata stays aligned for the design agent.

**3.0 Validation Checklist**
- Every requirement derived from the demand appears once, has a stable ID, follows EARS form, and is backed by ≥2 acceptance criteria.
- All sections from `templates/requirements.md` are present and populated (no empty headings or TODO text).
- Each requirement cites supporting evidence from the requirements brief or `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md`.
- `.codex-flow/runtime/speckit/spec-agent-output/<spec-name>.md` references the freshly generated requirements artifact and reflects any spec metadata changes required for downstream agents.
