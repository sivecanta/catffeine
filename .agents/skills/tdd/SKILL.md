---
name: tdd
description: >-
  Test-driven development workflow and seam discipline. Use when the user wants
  to build features or fix bugs test-first, mentions "red-green-refactor" or
  "TDD", or wants unit/integration tests added to Catffeine.
---

# Test-Driven Development

TDD is the red -> green loop. This skill is the reference that makes that loop produce tests worth keeping: what a good test is, where tests go, the anti-patterns, and the rules of the loop. Every section applies on every cycle — consult them before and during the loop, not after.

## What a good test is

Tests verify behavior through public interfaces, not implementation details. Code can change entirely; tests shouldn't. A good test reads like a specification — e.g., `"active agent overrides sleep policy to Working"` tells you exactly what capability exists — and survives refactors because it doesn't care about internal structure.

See [tests.md](./tests.md) for examples and [mocking.md](./mocking.md) for mocking guidelines.

## Seams — where tests go

A **seam** is the public boundary you test at: the interface where you observe behavior without reaching inside. Tests live at seams, never against internals.

**Test only at pre-agreed seams.** Before writing any test, write down the seams under test and confirm them with the user. No test is written at an unconfirmed seam. You can't test everything — agreeing on the seams up front is how testing effort lands on the critical paths and complex logic instead of every edge case.

In Catffeine, natural seams include:
- **State reconciliation (`src-tauri/src/lib.rs`)**: `AppStateInner::reconcile`, `StayAwakePolicy::duration`, timer expiration, and `active_agents` transitions returning `AppStateSnapshot`.
- **Watcher HTTP IPC (`src-tauri/src/watcher.rs`)**: `resolve_watcher_port` and loopback HTTP request/response handling (`POST /agent/start`, `POST /agent/stop`, `GET /status`).
- **Dock positioning math (`src-tauri/src/positioning.rs`)**: coordinate calculation for `BottomRight`, `BottomLeft`, and `FreeDrag` across scale factors.
- **OS Power Manager (`src-tauri/src/power.rs`)**: `PowerManager::prevent_sleep` / `allow_sleep` idempotency and state tracking.

Ask: "What's the public interface, and which seams should we test?"

## Anti-patterns

- **Implementation-coupled** — mocks internal collaborators, tests private helpers, or verifies through a side channel instead of the public return value / state snapshot. The tell: the test breaks when you refactor but behavior hasn't changed.
- **Tautological** — the assertion recomputes the expected value the way the code does (`assert_eq!( calc(a, b), a + b )`), so it passes by construction and can never disagree with the code. Expected values must come from an independent source of truth — a known-good literal, a worked example, or the spec.
- **Horizontal slicing** — writing all tests first, then all implementation. Bulk tests verify _imagined_ behavior: you test the _shape_ of things rather than observable behavior, the tests go insensitive to real changes, and you commit to test structure before understanding the implementation. Work in **vertical slices** instead — one test -> one implementation -> repeat, each test a **tracer bullet** that responds to what the last cycle taught you.

## Rules of the loop

- **Red before green.** Write the failing test first (run `cargo test --manifest-path src-tauri/Cargo.toml` to watch it fail), then write only enough code to pass it. Don't anticipate future tests or add speculative features.
- **One slice at a time.** One seam, one test, one minimal implementation per cycle.
- **Refactoring happens after green.** Only refactor once the test is passing and locks down the behavior at the seam.
