---
name: planner
description: >-
  Plan a significant Catffeine change before implementing it: trace the Rust
  and TypeScript codebase, compare options on long-term fit, grill the choice,
  and write a plan of bounded tasks to docs/plans/active. Use when asked to
  plan or design a feature, refactor, or cross-platform change touching
  multiple modules.
---

# Planner

Plan one significant change before anyone writes it. The run traces the code, compares options, grills the choice, and leaves a plan whose tasks an implementer can pick up as they stand. The run writes no application code.

A change you can state in one commit message needs no plan: say so and stop.

Options are judged on **fit**: how well the design holds up across Catffeine's cross-platform targets (Windows, macOS, Linux) and Tauri v2 architecture. The laziest implementation comes after the option is chosen. Activate `grilling` at step 4 and `ponytail` at step 6. Use `tdd` to place the test seams.

Catffeine references: `README.md` and `PROMPT.md` describe the core architecture and deliverables (`src-tauri/src/power.rs`, `positioning.rs`, `watcher.rs`, `lib.rs`, and `src/main.ts` + `styles.css`).

Copy this checklist into your reply and tick items off as you go:

- [ ] Frame
- [ ] Trace
- [ ] Options
- [ ] Grill
- [ ] Security & OS Boundaries
- [ ] Tasks
- [ ] Write

## 1. Frame

State the **destination** in one sentence: what is true when the work is done, observable from outside (a window/tray behaviour, an HTTP watcher endpoint response, an OS sleep-inhibition state). List what is out of scope.

Check whether the destination already exists. When it does, report that and stop.

Ask the user only what the code and docs cannot answer, all in one message.

Name the plan `<DD-MM-YY>-<title>` (today's date, kebab-case title). Its folder is `docs/plans/active/<DD-MM-YY>-<title>/`.

Done when: the destination, the out-of-scope list, and the plan's name are in your reply.

## 2. Trace

Read `README.md`, `PROMPT.md`, and any recent plans in `docs/plans/` that touched the same area. Then trace (or dispatch a `research` subagent via `invoke_subagent` to trace) the flow end to end across `src-tauri/src/` and `src/`. Record:

- every file on the path, with its role
- the seams where behaviour can change without editing callers
- helpers, types, and Tauri commands/events to reuse
- the tests covering the path, or their absence
- platform-specific `#[cfg(target_os = ...)]` branches affected

Done when: every file the change must touch is named with its role, and the covering tests are listed or their absence is stated.

## 3. Options

Draft up to 3 options that reach the destination. Each records the files it touches, the change in one sentence, its trade-off (what it makes easy and what hard), its risks across Windows/macOS/Linux, its cost in tasks, and what it defers with a trigger for upgrading it. Check outside facts (a Tauri v2 API, crate feature, or OS API constraint) before an option rests on them.

Present the options, the recommended one first, with the reason. Ask: "Which option should the plan build on?" and wait.

Done when: the user has picked an option.

## 4. Grill

Follow the `grilling` skill on the picked option: seam placement, state snapshot shape, IPC contract, platform failure modes, and which tests lock it down. Record decisions as they settle.

Done when: the user confirms the shared understanding.

## 5. Security & OS Boundaries

Only when the change touches a trust or OS boundary:
- the local HTTP watcher listener (`src-tauri/src/watcher.rs` — must stay bound to `127.0.0.1` loopback, bounded request reads, no arbitrary command execution),
- OS power inhibition (`src-tauri/src/power.rs` — Win32 FFI thread safety, `/usr/bin/caffeinate` child cleanup, Linux D-Bus cookie lifecycle),
- Tauri window capabilities (`src-tauri/tauri.conf.json`).

Review the design for those boundaries and record each guard with the test or check that proves it. Otherwise write "No trust or OS boundary touched." in the plan.

Done when: the Security & OS Boundaries section is filled, and every finding is a design change or a task with its guard and test.

## 6. Tasks

Follow `ponytail`. Break the option into tasks. Each carries:

- one change, in a sentence
- the files it may touch
- a **bound** the implementer can check: a named test passing, a build/clippy check succeeding, an endpoint response, or a function existing and called
- the tests that protect it, named before any implementation step (tests first)

Order tasks into **batches** of at most three. Tasks touching the same file go in different batches, and a task that needs another's result goes in a later batch. A deliberate shortcut names its ceiling and upgrade trigger.

Done when: every task has its files, bound, and tests; no file appears in two tasks of one batch; no task shares a batch with a task whose result it needs.

## 7. Write

Present the plan for approval. Once approved, write `docs/plans/active/<DD-MM-YY>-<title>/plan.md`:

```markdown
# <Title>

**Created:** <DD-MM-YY>
**Status:** planned
**Destination:** <the sentence from step 1>
**Out of scope:** <list>

## Decision

<The picked option, and why it fits best.>

**Rejected:** <each other option in one line, with the reason.>

## Tasks

### Batch 1

#### T1 - <change>

- **Files:** `<path>`, `<path>`
- **Bound:** <what the implementer checks to know it is done>
- **Tests:** <the tests that protect it>

## Security & OS Boundaries

- <boundary> - <what guards it, and the test/check that proves it>

## Shortcuts

- <what> - ceiling: <limit>; upgrade when: <trigger>

## Decisions from grilling

- <decision> - <reason>
```

Then print the path.

Done when: the plan file exists and its path is printed.
