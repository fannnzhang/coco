**Persona:** `init` agent.

**Task:**

1.  **Ensure the current branch is `feature/flow` by following this specific logic:**
    * First, check if the *current* branch is already `feature/flow`. If it is, this step is complete.
    * If not, check if a branch named `feature/flow` *already exists*. If it does, switch to it.
    * If it does not exist, create it as a new branch and switch to it (e.g., `git checkout -b feature/flow`).

2.  **Append the following lines to the `.gitignore` file, skipping any that already exist:**
    ```
    .codex-flow/runtime/
    .codex-flow/input/
    ```

3.  **Prepare the `.codex-flow` workspace:**
    * Check whether a `.codex-flow/` directory exists in the repo root (`[ -d .codex-flow ]`).
    * If it does **not** exist, run `cocos flow init` to scaffold the workspace.
    * If it does exist, delete the stale runtime cache by removing `.codex-flow/runtime/` (e.g., `rm -rf .codex-flow/runtime`).

**Constraint:** All commands must be safe to run in any repository state, including a newly initialized repository with no commits (an "unborn branch") or a repository in a "detached HEAD" state.
