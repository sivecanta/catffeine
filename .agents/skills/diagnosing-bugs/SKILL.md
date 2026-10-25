---
name: diagnosing-bugs
description: >-
  Diagnosis loop for hard bugs and performance regressions. Use when the user
  says "diagnose" or "debug this", or reports something broken, throwing,
  failing, or slow in Catffeine.
---

# Diagnosing Bugs

A discipline for hard bugs. Skip phases only when explicitly justified.

When exploring the codebase, read `README.md` and `PROMPT.md` to keep a clear mental model of Catffeine's modules (`src-tauri/src/lib.rs`, `power.rs`, `positioning.rs`, `watcher.rs`, and `src/main.ts`).

## Redact

This skill has you show commands, outputs, and captured artifacts. **Redact every secret first** — write `<REDACTED>` in its place. Build loops against env vars so credentials stay in the environment rather than in what you show.

If the redacted output is not enough to diagnose the bug, say so and ask the user.

## Phase 1 — Build a feedback loop

**This is the skill.** Everything else is mechanical. If you have a **tight** pass/fail signal for the bug — one that goes red on _this_ bug — you will find the cause; bisection, hypothesis-testing, and instrumentation all just consume it. If you don't have one, no amount of staring at code will save you.

Spend disproportionate effort here. **Be aggressive. Be creative. Refuse to give up.**

### Ways to construct one — try them in roughly this order

1. **Failing test** at whatever seam reaches the bug (`cargo test --manifest-path src-tauri/Cargo.toml`).
2. **Curl / HTTP script** against the local watcher server (`http://127.0.0.1:41905/status`, `/agent/start`, `/agent/stop`).
3. **CLI / OS state check** inspecting sleep inhibition state (`powercfg /requests` on Windows, `pmset -g assertions` on macOS, `dbus-send` / `systemd-inhibit` on Linux).
4. **Headless browser / preview check** against the Vite frontend (`npm run build` / `npm run dev`).
5. **Replay a captured trace.** Save a real HTTP payload or Tauri event payload and replay it through the handler in isolation.
6. **Throwaway harness.** Spin up a minimal Rust test or script exercising the bug code path with a single call.
7. **Property / fuzz loop.** If the bug is "sometimes wrong output" or a race in state reconciliation, run 1000 iterations in a test and assert invariants.
8. **Bisection harness.** If the bug appeared between two commits, automate the check for `git bisect run`.
9. **Differential loop.** Run the same input through two configurations and diff outputs.
10. **HITL bash script.** Last resort. If a human must interact with the desktop window, tray menu, or multi-monitor setup, drive _them_ with [`scripts/hitl-loop.template.sh`](./scripts/hitl-loop.template.sh) so the loop is still structured.

Build the right feedback loop, and the bug is 90% fixed.

### Tighten the loop

Treat the loop as a product. Once you have _a_ loop, **tighten** it:

- Can I make it faster? (Run a single targeted `cargo test <name>` instead of a full app rebuild.)
- Can I make the signal sharper? (Assert on the specific symptom in `AppStateSnapshot`, not just "didn't panic".)
- Can I make it more deterministic? (Pin durations/timestamps, isolate port numbers via `CATFFEINE_PORT`.)

### Non-deterministic bugs

The goal is not a clean repro but a **higher reproduction rate**. Loop the trigger 100x, parallelise requests against the watcher server, narrow timing windows, or inject yields. A 50%-flake bug is debuggable; 1% is not — keep raising the rate until it's debuggable.

### When you genuinely cannot build a loop

Stop and say so explicitly. List what you tried. Ask the user for: (a) access to the OS/environment that reproduces it, (b) a captured log/output, or (c) permission to add temporary instrumentation. Do **not** proceed to hypothesise without a loop.

### Completion criterion — a tight loop that goes red

Phase 1 is done when the loop is **tight** and **red-capable**: you can name **one command** — a script path, a test invocation, a curl — that you have **already run at least once** (show the invocation and its output, redacted), and that is:

- [ ] **Red-capable** — it drives the actual bug code path and asserts the **user's exact symptom**, so it can go red on this bug and green once fixed.
- [ ] **Deterministic** — same verdict every run (or a pinned, high reproduction rate for flaky bugs).
- [ ] **Fast** — seconds, not minutes.
- [ ] **Agent-runnable** — you can run it unattended; a human in the loop only via [`scripts/hitl-loop.template.sh`](./scripts/hitl-loop.template.sh).

If you catch yourself reading code to build a theory before this command exists, **stop — jumping straight to a hypothesis is the exact failure this skill prevents.** No red-capable command, no Phase 2.

## Phase 2 — Reproduce + minimise

Run the loop. Watch it go red — the bug appears.

Confirm:

- [ ] The loop produces the failure mode the **user** described — not a different failure that happens to be nearby. Wrong bug = wrong fix.
- [ ] The failure is reproducible across multiple runs.
- [ ] You have captured the exact symptom (error message, wrong state snapshot, misplaced window coordinates, leaked sleep lock) so later phases can verify the fix actually addresses it.

### Minimise

Once it's red, shrink the repro to the **smallest scenario that still goes red**. Cut inputs, callers, config, and steps **one at a time**, re-running the loop after each cut — keep only what's load-bearing for the failure.

Done when **every remaining element is load-bearing** — removing any one of them makes the loop go green.

Do not proceed until you have reproduced **and** minimised.

## Phase 3 — Hypothesise

Generate **3–5 ranked hypotheses** before testing any of them. Single-hypothesis generation anchors on the first plausible idea.

Each hypothesis must be **falsifiable**: state the prediction it makes.

> Format: "If `<X>` is the cause, then `<changing Y>` will make the bug disappear / `<changing Z>` will make it worse."

If you cannot state the prediction, the hypothesis is a vibe — discard or sharpen it.

**Show the ranked list to the user before testing.** Don't block on it — proceed with your ranking if the user is AFK.

## Phase 4 — Instrument

Each probe must map to a specific prediction from Phase 3. **Change one variable at a time.**

Tool preference:

1. **Targeted unit test / inspection** at the boundary.
2. **Targeted logs** (`eprintln!` / `console.debug`) at the boundaries that distinguish hypotheses.
3. Never "log everything and grep".

**Tag every debug log** with a unique prefix, e.g. `[DEBUG-a4f2]`. Cleanup at the end becomes a single grep. Untagged logs survive; tagged logs die.

**Perf branch.** For performance or CPU/battery regressions (e.g., excessive polling or repainting), establish a baseline measurement first, then bisect. Measure first, fix second.

## Phase 5 — Fix + regression test

Write the regression test **before the fix** — but only if there is a **correct seam** for it.

A correct seam is one where the test exercises the **real bug pattern** as it occurs at the call site. If no correct seam exists, note it explicitly and consider whether pure logic can be extracted into a testable function.

If a correct seam exists:

1. Turn the minimised repro into a failing test at that seam.
2. Watch it fail.
3. Apply the fix.
4. Watch it pass.
5. Re-run the Phase 1 feedback loop against the original (un-minimised) scenario.

## Phase 6 — Cleanup + post-mortem

Required before declaring done:

- [ ] Original repro no longer reproduces (re-run the Phase 1 loop)
- [ ] Regression test passes (or absence of seam is documented)
- [ ] All `[DEBUG-...]` instrumentation removed (`grep` the prefix)
- [ ] Throwaway prototypes deleted
- [ ] The hypothesis that turned out correct is stated in the summary / commit message — so the next debugger learns
