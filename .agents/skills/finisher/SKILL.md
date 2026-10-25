---
name: finisher
description: >-
  Close a Catffeine iteration: mark any active plan done and file it, sweep
  README.md and docs for stale references, and commit with a Conventional
  Commits message when the user agrees.
---

# Finisher

One finished change, with nothing left behind: the plan (if any) filed as done, no doc still describing what the change replaced, all checks green, and the work committed when the user agrees.

**The checks are the source of truth.** Nothing is committed while a check is red:
- `npm run build`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`

Copy this checklist into your reply and tick items off as you go:

- [ ] Locate
- [ ] File
- [ ] Sweep
- [ ] Commit
- [ ] Report

## 1. Locate

Run `git status --short` and `git diff --stat`. Find the active plan in `docs/plans/active/` if the user names one or if one matches the change. A change can have no plan; then step 2 is skipped.

Done when: the changed files and the plan's path (or its absence) are listed.

## 2. File

If an active plan exists for this change:
- Set the plan's `**Status:** done`.
- Create `docs/plans/done/` when missing and move the plan's folder there (`git mv`).
- Fix any links that pointed at the old path (`git grep`).

Done when: `docs/plans/active` holds nothing of this plan and no link points at the old path.

## 3. Sweep

List each **retired value** the change removed, renamed, or bumped (a Tauri command, event name, HTTP watcher route, port, env var, policy variant, CSS state attribute, or file path) with its replacement. Search the repo (`git grep`) for each one and return every hit with file and line.

Sort each hit:

- **Stale** — it describes the current state (`README.md`, `PROMPT.md`, comments, config files). Update it.
- **History** — it records the past on purpose (`docs/plans/done/`, commit history). Leave it.
- **Generated** — regenerate with its tool (`dist/`).

When the sweep changed code or config, re-run the checks.

Done when: every hit sits in one bucket, the stale ones are updated, and the checks are green.

## 4. Commit

Show the user the files to stage and a proposed message following Conventional Commits 1.0.0: `<type>(<scope>): <description>`, lower case, imperative, no period. Commit only when the user says yes.

Done when: the commit exists, or the user declined and the proposed message is in your reply.

## 5. Report

In your reply: the plan's new path (if any), each stale hit that was updated, the commit hash (or that none was made), and anything not verified on the current OS.
