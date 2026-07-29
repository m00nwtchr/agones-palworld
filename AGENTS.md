# AGENTS.md — agones-palworld

## Development methodology

**Always use the superpowers `subagent-driven-development` flow for implementation work.**

When a feature or fix is requested:

1. **Brainstorming** (for new features) — assemble the design spec under `docs/superpowers/specs/`.
2. **Writing-plans** — produce the implementation plan under `docs/superpowers/plans/`.
3. **Subagent-driven-development** — execute the plan task-by-task. Each task is dispatched to a fresh subagent. I do not write code directly; I dispatch, review, and move on.
4. **Honcho** — after each task, persist any durable conventions or non-obvious decisions via `honcho_create_conclusion`.

The dispatcher (this session) does not edit code, write tests, or run builds. The implementer subagent does. The reviewer subagent verifies.

## Quality gates

Every task must pass before it is committed:

- `cargo fmt --all --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `helm lint helm` (when `helm/` changes)
- `shellcheck scripts/*.sh` (when shell scripts change)

Reviewed commits must reference the plan task in the message body where useful.

## Git conventions

- **Never override the committer identity.** The user's global `user.name` / `user.email` is authoritative. If a commit requires a non-default identity, surface it to the user and wait.
- **GPG signing.** The user's global `commit.gpgsign=true`. If the agent environment cannot unlock the GPG key (e.g., unattended shell), use `git -c commit.gpgsign=false commit` for that one commit only. Do not modify the user's global config.

## Existing reference

- Config-patching script sourced from `homelab-cluster/kubernetes/apps/games/palworld/app/resources/patch-palworld-settings.sh`. Vendored verbatim into `helm/files/`. Do not modify in this repo.

## Full development flow

**Always run the entire superpowers dev flow for every round of changes:**

- **brainstorming** — Activates before writing code. Refines rough ideas through questions, explores alternatives, presents design in sections for validation. Saves design document to `docs/superpowers/specs/`.
- **using-git-worktrees** — Activates after design approval. Creates isolated workspace on a new branch, runs project setup, verifies clean test baseline.
- **writing-plans** — Activates with approved design. Breaks work into bite-sized tasks (2–5 minutes each). Every task has exact file paths, complete code, verification steps.
- **subagent-driven-development** (or executing-plans) — Activates with plan. Dispatches a fresh subagent per task with two-stage review (spec compliance, then code quality), or executes in batches with human checkpoints.
- **test-driven-development** — Activates during implementation. Enforces RED-GREEN-REFACTOR: write failing test, watch it fail, write minimal code, watch it pass, commit. Deletes code written before tests.
- **requesting-code-review** — Activates between tasks. Reviews against plan, reports issues by severity. Critical issues block progress.
- **finishing-a-development-branch** — Activates when tasks complete. Verifies tests, presents options (merge / PR / keep / discard), cleans up worktree.
