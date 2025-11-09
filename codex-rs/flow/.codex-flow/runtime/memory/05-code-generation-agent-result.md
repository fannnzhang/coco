**ADR Added**
- Captured the full FR/NFR traceability, precedence model, module responsibilities, and verification strategy for the reasoning controls program inside `docs/adr/ADR-001-reasoning-trace.md:1`. This includes explicit references to the specification, plan artifacts, planned diagrams, and the downstream files/tests each requirement touches so later tasks (schema updates, diagrams, resolver, templates) can point back to an approved scope.

Tests were not run (documentation-only change).

**Next steps**
1. Use the ADR as the cited source while implementing I1.T2 schema changes and ERD updates.
2. Update the ADR’s “Related Artifacts” section once the reasoning diagrams land so reviewers can navigate between design docs quickly.