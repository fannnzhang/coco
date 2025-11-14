# Resume Rollout Runbook

This document tracks the phased rollout of workflow resume support across
`codex-flow`. Each phase requires sign-off from the Flow Runtime Team and the
on-call operator pairing the rollout.

| Phase | Goal | Status | Approval |
| --- | --- | --- | --- |
| Phase A | Mock-only state persistence behind `runtime.resume.enabled` | ✅ | 2025-11-11 – Flow Runtime Team |
| Phase B | Real-engine resume + token ledger observability | ✅ | 2025-11-11 – Flow Runtime Team |
| Phase C | Default-on resume, migrations, cleanup tooling | ✅ | 2025-11-11 – Flow Runtime Team |

## Steady-state operations (Phase C)

- All `codex-flow run` executions (mock or real) write
  `.codex-flow/runtime/state/<workflow>/<run-id>.resume.json` automatically.
- `codex-flow resume <workflow>.toml --run-id <id>` is GA; no flags required.
- `CODEX_RESUME_DISABLED=1` acts as the emergency kill switch. Setting the env
  variable disables new state writes and prevents `resume` from running; unset
  it before resuming normal operations.
- Run `codex-flow state prune --days <N>` weekly to delete stale runs. Capture
  the printed disk-usage summary in the maintenance log.

## Monitoring & verification

1. Execute `scripts/resume-smoke.sh` before and after any release touching the
   runner. The script now asserts that state files exist for both mock and real
   runs and exercises `codex-flow resume` without hidden flags.
2. Watch the `token_usage` totals emitted by the CLI (verbose mode) and the
   `TokenLedger` dashboards. Real-mode replays should only re-run steps flagged
   `needs_real`.
3. Keep an eye on `.codex-flow/runtime/state` growth. If reclaimable storage is
   <10% after a prune run, increase the prune cadence or days threshold and
   document the change here.

## Rollback procedure

1. Export `CODEX_RESUME_DISABLED=1` in the affected environment (shell profile
   or process supervisor). Announce the rollback in `#flow-runtime`.
2. Re-run `scripts/resume-smoke.sh` to confirm that resume now errors early; the
   absence of state writes is expected during rollback.
3. Investigate/patch the regression. Once fixed, remove the env var, run the
   smoke script again, and capture the console output in the release ticket.
4. If any `.resume.json` files were generated with an experimental schema, move
   them aside—the loader already snapshots them as `*.corrupt-<timestamp>` for
   postmortem debugging.

Document every rollout/rollback in this file with the date, approver, and links
to supporting tickets.
