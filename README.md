# Catffeine

A cozy, lightweight cross-platform (macOS, Windows, Linux) pixel-art desktop companion cat that lives on your desktop, taskbar, or menu bar, preventing your computer from going to sleep indefinitely, on a timer (`30m` / `1h` / `2h`), or dynamically while long-running AI agents and CLI processes are active.

## Architecture

- **Framework**: Tauri v2 + Rust (`src-tauri/`)
- **Frontend**: Vanilla TypeScript + Pixel-Art SVG/CSS (`src/`, `index.html`)
- **Window**: Frameless, transparent, `alwaysOnTop: true` by default, `skipTaskbar: true`, with system tray (Windows) and menu bar (`ActivationPolicy::Accessory` on macOS) integration

### Core Modules

1. **OS Power Management (`src-tauri/src/power.rs`)**
   - `PowerManager::prevent_sleep()` / `PowerManager::allow_sleep()`
   - **Windows**: `windows-sys` (`SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)`) executed on a dedicated OS thread.
   - **macOS**: Spawns and supervises a `/usr/bin/caffeinate -di` child process.
   - **Linux**: Acquires and releases a Freedesktop ScreenSaver / PowerManagement inhibition cookie over D-Bus (`zbus`).

2. **Window Positioning, Dragging & Pinning (`src-tauri/src/positioning.rs`)**
   - Queries monitor geometry (`window.current_monitor()`) and HiDPI scale factor.
   - Modes: **Bottom Right**, **Bottom Left**, **Unpin & Move (`free-drag`)**, and **Pin Here (`pinned`)** to lock the cat at any custom screen coordinate.
   - Controllable via the right-click context menu on the cat and the OS system tray / menu bar icon.

3. **Agent / Process Watcher HTTP IPC (`src-tauri/src/watcher.rs`)**
   - Lightweight loopback HTTP server on `127.0.0.1:41905` (configurable via `CATFFEINE_PORT`):
     - `POST http://127.0.0.1:41905/agent/start` — Increments active job counter, forces Keep-Awake ON, switches cat to `"working"` animation.
     - `POST http://127.0.0.1:41905/agent/stop` — Decrements job counter; when count hits `0`, restores the user's selected sleep policy and transitions back to `"sleeping"` or `"awake"`.
     - `POST http://127.0.0.1:41905/window/show` — Unhides and brings the Catffeine window to the front.
     - `POST http://127.0.0.1:41905/window/hide` — Hides the cat into the system tray / mini-pill mode.
     - `GET http://127.0.0.1:41905/status` — Returns JSON state snapshot (`AppStateSnapshot`).

4. **Frontend Pixel-Art State Machine (`src/main.ts`, `src/styles.css`, `index.html`)**
   - Visual states: `sleeping` (keep-awake OFF), `awake` (keep-awake ON), `working` (agent running with coding glasses & laptop).
   - Hide / Unhide support via right-click menu, left-clicking the Windows system tray / macOS menu bar icon, or the compact `Show Cat` pill.

## Development & Building

```bash
npm install
npm run tauri dev

# Run checks & unit tests
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Cross-compile standalone Windows catffeine.exe from WSL
npm run build:win
```
