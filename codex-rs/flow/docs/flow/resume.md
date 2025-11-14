# Workflow Resume & State Management

The `codex-flow` CLI now persists every workflow run (mock and real) to
`.codex-flow/runtime/state/<workflow>/<run-id>.resume.json`. These files contain
the per-step execution history, aggregate token usage, and a resume pointer so
interrupted runs can continue without repeating completed steps.

## Running workflows

- `codex-flow run ./workflow.toml --run-id 20251111T120000Z` writes the run
  history automatically. When `--run-id` is omitted the CLI generates a UTC
  timestamp id and prints it at the end of the run.
- `codex-flow resume ./workflow.toml --run-id 20251111T120000Z` restarts from
  the saved resume pointer. The `--mock/--no-mock` switches still apply and the
  runner automatically replays any steps that were marked `needs_real` or have
  missing debug logs.
- `codex-flow run ... --resume-from ./state.json` can bootstrap a fresh run from
  a previously exported state file; the new execution inherits the stored
  `resume_pointer`, token usage, and per-step metadata.

Resume is enabled by default. Set the hidden
`CODEX_RESUME_DISABLED=1` environment variable only during emergency rollbacks
if state persistence must be bypassed.

## Token accounting

State files now record a workflow-level `token_usage` object and each
`StepState` includes an optional `token_delta`. These values are populated by
the token ledger so operators can audit prompt/completion tokens and cost for
every run. When resuming in real-engine mode the runner inspects earlier steps
and re-executes any that still require live data to keep those totals accurate.

## Cleaning up state

Use the new pruning command to remove stale runs and keep disk usage in check:

```bash
codex-flow state prune --days 30
```

The command scans `.codex-flow/runtime/state`, removes `*.resume.json` files
older than the threshold, reports before/after disk usage, and reinstalls the
README template so operators always see the latest guidance.
