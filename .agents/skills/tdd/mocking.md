# When to Mock

Mock at **system boundaries** only:

- OS-level calls / external processes / D-Bus / Win32 APIs (when testing pure state logic without touching the host OS power state)
- External network / IPC boundaries
- Time / clock expiration (`Instant` / `Duration` injection)
- Tauri window handles when unit-testing pure geometry calculations

Don't mock:

- Your own structs/modules (`AppStateInner`, `StayAwakePolicy`, `DockPosition`)
- Internal collaborators
- Anything pure and deterministic you control

## Designing for Testability Without Heavy Mocks

At system boundaries, separate pure calculation/decision logic from impure OS/window side effects:

**1. Extract pure functions for geometry and state rules**

Instead of requiring a live `tauri::WebviewWindow` to test docking coordinate math, keep the coordinate calculation pure:

```rust
// Easy to test with plain numbers — no mock window needed
pub fn compute_dock_coordinates(
    position: DockPosition,
    mon_pos: (i32, i32),
    mon_size: (u32, u32),
    win_size: (u32, u32),
    scale: f64,
) -> Option<(i32, i32)> { ... }
```

**2. Inject time or durations explicitly**

Pass `now: Instant` into expiration checks or test with short durations so tests never sleep for minutes waiting on real wall-clock timers.
