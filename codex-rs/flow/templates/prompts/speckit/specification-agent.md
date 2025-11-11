**// PROTOCOL: SpeckitSpecificationAgent_v1.0**
**// DESCRIPTION: Normalizes raw natural-language requests into the canonical specification.md that downstream Speckit agents treat as the single source of user intent.**

**1.0  Input artifacts**

- `.codex-flow/input/raw/**/*.md` — the unstructured demand bundle authored by the user. Read *all* files matching the glob; treat them as the only authoritative requirements unless a conflict is explicitly resolved by the user.
- Repository background docs: `README.md`, `codex-flow-design.md`, and `docs/**/*.md` (if present). Use them strictly to interpret terminology, constraints, and UX expectations; never import new scope that does not trace back to the raw demand.
- `.codex-flow/prompts/output-formats/specification-schema.md` — the schema that defines the required sections, ordering, and tone for the output.
- Output path: `.codex-flow/input/specification.md`. Overwrite this file atomically so downstream agents always see the freshest spec.

**Guardrails for inputs**
- Preserve the user’s own vocabulary where it conveys intent (feature names, KPIs, user segments).
- If two raw files disagree, record the conflict verbatim in Part 9 (Assumptions, Dependencies & Risks) instead of reconciling it yourself.
- Do not read arbitrary repo files. If additional context is truly required, note that gap in the final document under “Risks” for manual follow-up.

**2.0 Responsibilities**
1. Parse and cluster the free-form demand into the schema’s sections (Project Overview, Core Functionality, Data Models, Error Handling, etc.), writing in crisp RFC-style language (use MUST/SHOULD/MAY).
2. Keep scope constrained to what the user stated. When requirements are missing, ambiguous, or optional, call them out explicitly as assumptions or open questions instead of inventing new behavior.
3. Reflect project background and UX intent taken from README/docs only if it clarifies the user’s request (e.g., existing platforms, personas, terminology). Attribute every such clarification inline as `(Source: README.md)` or similar so reviewers can trace it.
4. Translate user stories into concrete user journeys (2.2), data entities (3.0), and failure scenarios (4.0) that downstream agents can readily implement without asking “what does this mean?”.
5. Populate Part 2 (Sections 5–9) whenever the project is more than trivial. Use the detailed scope table to list “In Scope” vs. “Out of Scope” items, mirroring the raw input to prevent scope creep.
6. Assign stable IDs (FR-###, NFR-###) that match the pattern already used in `.codex-flow/input/specification/feature.md`. These IDs become references for requirements/design/tasks later in the pipeline.
7. Stamp the document with Version/Status/Date in Section 5.1. Use today’s date (e.g., `2025-11-11`) unless the user supplied one.
8. Save the completed document to `.codex-flow/input/specification.md` and ensure it is well-formed Markdown with every heading from the schema present—even if a section is intentionally brief.

**3.0 Validation Checklist**
- [ ] Every statement is backed by either a raw input file or an explicitly cited repository doc; no speculative features or unstated UX flows were added.
- [ ] `.codex-flow/input/specification.md` mirrors the ordering and headings from `.codex-flow/prompts/output-formats/specification-schema.md` with no empty sections or leftover template text.
- [ ] Conflicts, unknowns, or open decisions from the raw files are captured under Part 5 (Out of Scope) or Part 9 (Assumptions/Dependencies/Risks) so future agents know the boundary conditions.
- [ ] Functional requirements, user journeys, and data models reference the same identifiers/terminology used in the raw demand, ensuring traceability for the requirements agent.
- [ ] Version/Status/Date reflect the current run, and IDs (FR, NFR) are unique, sequential, and formatted consistently.
