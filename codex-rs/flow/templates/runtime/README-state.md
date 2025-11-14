# Workflow Resume State

This directory stores per-workflow run state captured by `codex-flow`.

- Files follow the naming pattern `<workflow>/<run-id>.resume.json` and are
  written atomically with a matching `.tmp` suffix during updates.
- Schema version `2` defines `WorkflowRunState`, `StepState`, and
  `TokenUsage`. New versions will include on-disk migrations before rollout.
- These files power `codex-flow resume` so do **not** edit them manually unless
  instructed; instead, rerun the workflow to regenerate state.
- Use `codex-flow state prune --days <N>` to remove stale runs and see a disk
  usage summary that keeps this directory tidy.

If a file becomes corrupted, the runner keeps a `.corrupt-<timestamp>` copy for
inspection. To regenerate the state, re-run the associated workflow with the
same `--run-id` in mock mode.
