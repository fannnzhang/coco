#!/usr/bin/env bash
set -euo pipefail

WORKFLOW=${1:-.codex-flow/workflows/codex-flow-development.workflow.toml}
RUN_ID=${2:-$(date -u +%Y%m%dT%H%M%SZ)}

echo "[resume-smoke] workflow: $WORKFLOW"
echo "[resume-smoke] run-id: $RUN_ID"

codex-flow run "$WORKFLOW" --mock --run-id "$RUN_ID" --verbose
STATE_PATH=$(find .codex-flow/runtime/state -name "${RUN_ID}.resume.json" -print -quit || true)
if [[ -z "$STATE_PATH" ]]; then
  echo "[resume-smoke] ERROR: missing state file after mock run" >&2
  exit 1
fi
echo "[resume-smoke] state recorded at $STATE_PATH"
codex-flow resume "$WORKFLOW" --run-id "$RUN_ID" --mock --verbose

REAL_RUN_ID="${RUN_ID}-real"
echo "[resume-smoke] real run-id: $REAL_RUN_ID"
codex-flow run "$WORKFLOW" --no-mock --run-id "$REAL_RUN_ID" --verbose
REAL_STATE_PATH=$(find .codex-flow/runtime/state -name "${REAL_RUN_ID}.resume.json" -print -quit || true)
if [[ -z "$REAL_STATE_PATH" ]]; then
  echo "[resume-smoke] ERROR: missing state file after real run" >&2
  exit 1
fi
echo "[resume-smoke] state recorded at $REAL_STATE_PATH"
codex-flow resume "$WORKFLOW" --run-id "$REAL_RUN_ID" --no-mock --verbose
