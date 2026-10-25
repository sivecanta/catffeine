# Catffeine

A cozy, lightweight cross-platform (**Windows**, **macOS**, **Linux**) pixel-art desktop companion cat that lives on your desktop, Windows System Tray, or macOS Menu Bar — preventing your computer from going to sleep indefinitely, on a timer (`30m` / `1h` / `2h`), or dynamically while long-running AI agents and CLI processes are active.

---

## Supported Platforms & Architectures

| OS | Architecture | Artifact / Format | Sleep Prevention Mechanism |
| :--- | :--- | :--- | :--- |
| **Windows 10 / 11** | `x86_64` (64-bit) | `catffeine.exe` (Standalone portable binary) | Win32 `SetThreadExecutionState(ES_CONTINUOUS \| ES_SYSTEM_REQUIRED \| ES_DISPLAY_REQUIRED)` |
| **macOS** | `aarch64` (Apple Silicon) & `x86_64` (Intel) | `Catffeine.app` / `.dmg` | Spawns and supervises `/usr/bin/caffeinate -di` |
| **Linux** | `x86_64` (64-bit) | Binary / `.deb` / `.AppImage` | Freedesktop D-Bus `org.freedesktop.ScreenSaver` / `PowerManagement` |

---

## Quick Start: Download from GitHub Releases

1. Go to the [**Releases**](../../releases) page and download the binary for your platform:
   - **Windows (`x86_64`, 64-bit)**: Download `catffeine.exe`.
   - **macOS**: Download `Catffeine.dmg` / `Catffeine.app`.

### Running Unsigned Release Binaries

Because release binaries are community-built without a paid EV / Apple Developer ID certificate, your OS may flag files downloaded via a web browser:

- **Windows 10 / 11 (SmartScreen / Smart App Control)**:
  - Before double-clicking a downloaded `catffeine.exe`, unblock the browser's Mark-of-the-Web in PowerShell:
    ```powershell
    Unblock-File -Path .\catffeine.exe
    ```
  - Or if running from WSL, launch it directly from the terminal (`./catffeine.exe`) or copy it via raw bytes:
    ```powershell
    [IO.File]::WriteAllBytes("$HOME\Desktop\catffeine.exe", [IO.File]::ReadAllBytes("\\wsl.localhost\Ubuntu\home\kaizen\projects\git\catffeine\catffeine.exe"))
    ```
- **macOS (Gatekeeper)**:
  - Clear the quarantine attribute once in Terminal before opening:
    ```bash
    xattr -cr /Applications/Catffeine.app
    ```

---

## Launching & Building from Terminal

### 1. Run in Development Mode
```bash
npm install
npm run tauri dev
```

### 2. Build Standalone Windows `.exe` (`x86_64-pc-windows-msvc`) from WSL / Linux
```bash
npm run build:win
```
This cross-compiles the optimized standalone Windows executable to `./catffeine.exe` (and `src-tauri/target/x86_64-pc-windows-msvc/release/catffeine.exe`).

You can launch it directly from your WSL or Windows terminal:
```bash
./catffeine.exe
```

### 3. Build Native Release Bundle (macOS / Linux / Windows)
```bash
npx tauri build
```

---

## Controls & Features

- **Left-Click on Cat**: Toggle between **Sleeping** (`off`) and **Staying Awake** (`indefinite`).
- **Right-Click on Cat (or System Tray / Menu Bar Icon)**:
  - **Stay Awake**: `Indefinite` / `30 Minutes` / `1 Hour` / `2 Hours` / `Off (Allow Sleep)`
  - **Position & Pin**:
    - `Unpin & Move`: Drag the cat anywhere on your screen.
    - `Pin Here (Lock)`: Lock the cat at its current custom screen coordinates (also available via the one-click **`Pin Here`** badge while in Free Drag mode).
    - `Bottom Right` / `Bottom Left`: Snap above the taskbar/dock corners.
  - **Always on Top**: Keep the cat floating above all windows (enabled by default).
  - **Launch on Startup**: Automatically start Catffeine when you log into Windows (`HKCU\...\Run` via `%LOCALAPPDATA%\Catffeine\catffeine.exe`), macOS (`~/Library/LaunchAgents/com.catffeine.desktop.plist`), or Linux (`~/.config/autostart/catffeine.desktop`).
  - **Hide / Show Cat**: Hide the desktop window into the Windows System Tray / macOS Menu Bar. Left-click the tray/menu-bar cat icon (or click `Show Cat`) to unhide anytime.

---

## Agent / CLI Watcher HTTP API (`127.0.0.1:41905`)

Catffeine runs a lightweight loopback HTTP listener on `127.0.0.1:41905` (override port with `CATFFEINE_PORT`) so AI agents and long-running CLI scripts can automatically keep your computer awake while they work:

```bash
# Start an active agent job (forces Keep-Awake ON + switches cat to "Working" with glasses & laptop)
curl -X POST http://127.0.0.1:41905/agent/start

# Stop an active agent job (restores user's sleep policy when active_agents reaches 0)
curl -X POST http://127.0.0.1:41905/agent/stop

# Show or hide the cat window from terminal
curl -X POST http://127.0.0.1:41905/window/show
curl -X POST http://127.0.0.1:41905/window/hide

# Query current state snapshot (JSON)
curl http://127.0.0.1:41905/status
```

---

## Verification & Tests

```bash
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```
