# codex-flow changelog

## 2025-11-11 – Default resume rollout (Phase C)

- Removed the `runtime.resume.enabled` flag so every `codex-flow run` writes
  state by default; `CODEX_RESUME_DISABLED` now serves as the emergency kill
  switch and `codex-flow resume` no longer needs `--experimental-resume`.
- Added workflow state migrations (schema v2) that backfill `token_usage` from
  historical step deltas and persist upgraded files automatically.
- Introduced `codex-flow state prune --days <N>` to delete stale
  `*.resume.json` files, refresh the README template, and print disk-usage
  summaries for operators.
- Published `docs/flow/resume.md` plus updated specifications to mark FR-004 as
  delivered and document the new CLI surface area.

## 2025-11-11 – Real-engine resume availability (Phase B)

- Added the `engine::metrics::TokenLedger` to aggregate token deltas and workflow-level usage with pricing so state files capture cost data.
- `codex-flow resume` now supports real engines, automatically rerunning steps whose debug logs are missing and honoring `needs_real` markers.
- `codex-flow run` accepts `--resume-from <state.json>` to continue a workflow in a new process while skipping previously completed steps once the source state passes validation.
- Verbose output (`--verbose`) for both `run` and `resume` now prints the last completed step, current resume pointer, and token delta; `scripts/resume-smoke.sh` exercises both mock and real flows through the experimental flag.
