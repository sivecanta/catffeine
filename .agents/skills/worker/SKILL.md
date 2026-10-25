---
name: worker
description: >-
  Build one Catffeine change from a plan or a description: grill, split into
  bounded tasks, implement or delegate to subagents, run Rust and TypeScript
  checks, and review the diff locally until clean. Works on the current branch.
---

# Worker

One change, built and reviewed on the current branch. The run ends with a clean local review and green checks. Filing the plan, sweeping docs, and committing belong to `finisher`.

The change arrives as a plain description, or as a plan in `docs/plans/active/` whose decisions are settled. Steps 1 and 3 read what the plan settles instead of redoing it.

Keep `ponytail` active for the whole run. Follow `tdd` for every task: tests first where a seam exists.

**The checks are the source of truth.** Green or red is decided by Catffeine's build and test commands:

- Frontend check: `npm run build` (runs `tsc && vite build`)
- Rust check & tests: `cargo test --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`

On red, fix the implementation and re-run. An edited assertion, an added `#[ignore]`, or a loosened type counts as red. After three red runs on the same failure, stop and report the command and its last output. Skip a side that the change did not touch only if you explicitly state so in the report. Don't commit unless the user asks.

Copy this checklist into your reply and tick items off as you go; steps 5 and 6 repeat per round:

- [ ] Grill
- [ ] Base
- [ ] Split
- [ ] Execute
- [ ] Secure
- [ ] Review
- [ ] Report

## 1. Grill

**From a plan:** read it. Its decisions are settled. Check that the files it names still exist; ask the user only about what drifted.

**From a description:** follow `grilling` on the change before touching code when requirements or design choices are ambiguous. Scope, the done condition, and whether each part needs to exist are the user's decisions.

Done when: the plan is read and still current, or the user confirms the shared understanding.

## 2. Base

Record the review baseline: `git rev-parse HEAD` is `$BASE`. The working tree may already hold uncommitted changes; run `git status --short` and note which files were dirty before you start, so the review can tell your work from prior changes.

Done when: `$BASE` and the dirty-before list are in your reply.

## 3. Split

**From a plan that lists tasks:** those tasks and batches are the split.

**From a description:** break the change into tasks, each with a bound the implementer can check (a passing test, a clean build, a named function/command existing and wired). State the list before executing.

Done when: every task is named with its own bound, and together they cover the change.

## 4. Execute

For multi-task batches touching independent files, you may either execute sequentially or spawn `self` subagents via `invoke_subagent` (at most three at a time, with non-overlapping files per batch).

For each task:
- Follow `ponytail` and write the failing test first at the agreed seam (`tdd`).
- Verify the task's bound.

Once all tasks in the change are integrated, run the full check suite (`npm run build`, `cargo test`, `cargo clippy`).

Done when: every task's bound is met, and every check command ran green (show each command and its result).

## 5. Secure

Only for a change that touches a trust or OS boundary (watcher loopback HTTP server in `src-tauri/src/watcher.rs`, OS sleep inhibition / FFI / child processes / D-Bus in `src-tauri/src/power.rs`, or Tauri window permissions in `src-tauri/tauri.conf.json`). Inspect the diff across those boundaries and sort any finding:
- **Confirmed** — fix it and re-run the checks.
- **Wrong** — name the mechanism that disproves it.
- **Deferred** — state where it belongs.

Otherwise write "No trust or OS boundary touched."

Done when: every finding is in one bucket with its evidence, and the checks are green.

## 6. Review

Review `git diff $BASE` carefully across all changed files. Treat each potential issue (race conditions on `AppState` mutex, platform `#![cfg]` mismatches across Windows/macOS/Linux, unclosed resources, click-through hitbox regressions, or unnecessary bloat) as a hypothesis until the code confirms it. Sort each as **Confirmed**, **Wrong**, or **Deferred**. Fix the Confirmed ones, re-run the checks, and review again.

- **No findings** — the work is clean. Go to step 7.
- **Only Wrong or Deferred findings** — report each with its reason and stop; the user decides.
- **After five rounds** — stop and report what the fifth round found.

Done when: the last round found nothing, or every remaining finding is answered, or the cap stopped it.

## 7. Report

In your response: what was built per task, the check commands with their results, the review rounds with totals per bucket, anything Deferred, and anything not verified (e.g., whether cross-platform `macos`/`linux`/`windows` branches or live desktop window behaviour were tested on the host OS). Mention that `finisher` can be used next to close out the iteration and commit.
