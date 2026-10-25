//! Cross-platform OS Power Management Layer for Catffeine.
//!
//! Provides a unified [`PowerManager`] abstraction across:
//! - **Windows**: Calls `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)`
//!   on a dedicated OS thread (since Windows thread execution state is per-thread).
//! - **macOS**: Spawns and manages a `/usr/bin/caffeinate -di` child process.
//! - **Linux**: Acquires and releases a Freedesktop ScreenSaver inhibition cookie via D-Bus (`zbus`).

#[cfg(target_os = "macos")]
use std::process::{Child, Command, Stdio};

#[cfg(target_os = "windows")]
use std::sync::mpsc::{self, SyncSender};

/// Unified cross-platform sleep inhibitor.
pub struct PowerManager {
    preventing_sleep: bool,
    #[cfg(test)]
    mock_mode: bool,

    #[cfg(target_os = "windows")]
    win_tx: Option<SyncSender<WinPowerCmd>>,

    #[cfg(target_os = "macos")]
    caffeinate_child: Option<Child>,

    #[cfg(target_os = "linux")]
    linux_inhibit: Option<LinuxInhibitHandle>,
}

#[cfg(target_os = "windows")]
enum WinPowerCmd {
    Prevent(SyncSender<Result<(), String>>),
    Allow(SyncSender<Result<(), String>>),
    Shutdown,
}

#[cfg(target_os = "linux")]
struct LinuxInhibitHandle {
    connection: zbus::blocking::Connection,
    destination: &'static str,
    path: &'static str,
    interface: &'static str,
    cookie: u32,
}

impl PowerManager {
    /// Creates a new `PowerManager` in the default sleep-allowed state.
    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        let win_tx = Some(spawn_windows_power_thread());

        Self {
            preventing_sleep: false,
            #[cfg(test)]
            mock_mode: false,
            #[cfg(target_os = "windows")]
            win_tx,
            #[cfg(target_os = "macos")]
            caffeinate_child: None,
            #[cfg(target_os = "linux")]
            linux_inhibit: None,
        }
    }

    #[cfg(test)]
    pub fn new_mock() -> Self {
        Self {
            preventing_sleep: false,
            mock_mode: true,
            #[cfg(target_os = "windows")]
            win_tx: None,
            #[cfg(target_os = "macos")]
            caffeinate_child: None,
            #[cfg(target_os = "linux")]
            linux_inhibit: None,
        }
    }

    /// Returns `true` if system and display sleep are currently inhibited.
    pub fn is_preventing_sleep(&self) -> bool {
        self.preventing_sleep
    }

    /// Prevents the operating system and display from going to sleep.
    /// Idempotent if sleep is already being prevented.
    pub fn prevent_sleep(&mut self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if let Some(child) = self.caffeinate_child.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.caffeinate_child = None;
                self.preventing_sleep = false;
            }
        }

        if self.preventing_sleep {
            return Ok(());
        }

        #[cfg(test)]
        if self.mock_mode {
            self.preventing_sleep = true;
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        if let Some(tx) = &self.win_tx {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(WinPowerCmd::Prevent(reply_tx))
                .map_err(|e| format!("Windows power thread channel error: {e}"))?;
            reply_rx
                .recv()
                .map_err(|e| format!("Windows power thread response error: {e}"))??;
        }

        #[cfg(target_os = "macos")]
        {
            let child = Command::new("/usr/bin/caffeinate")
                .arg("-di")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Failed to spawn /usr/bin/caffeinate: {e}"))?;
            self.caffeinate_child = Some(child);
        }

        #[cfg(target_os = "linux")]
        {
            self.linux_inhibit = Some(acquire_linux_inhibit()?);
        }

        self.preventing_sleep = true;
        Ok(())
    }

    /// Releases any active sleep inhibition and restores the OS default sleep policy.
    /// Idempotent if sleep is already allowed.
    pub fn allow_sleep(&mut self) -> Result<(), String> {
        if !self.preventing_sleep {
            return Ok(());
        }

        #[cfg(test)]
        if self.mock_mode {
            self.preventing_sleep = false;
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        if let Some(tx) = &self.win_tx {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.send(WinPowerCmd::Allow(reply_tx))
                .map_err(|e| format!("Windows power thread channel error: {e}"))?;
            reply_rx
                .recv()
                .map_err(|e| format!("Windows power thread response error: {e}"))??;
        }

        #[cfg(target_os = "macos")]
        if let Some(mut child) = self.caffeinate_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        #[cfg(target_os = "linux")]
        if let Some(handle) = self.linux_inhibit.take() {
            release_linux_inhibit(handle)?;
        }

        self.preventing_sleep = false;
        Ok(())
    }
}

impl Default for PowerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PowerManager {
    fn drop(&mut self) {
        let _ = self.allow_sleep();
        #[cfg(target_os = "windows")]
        if let Some(tx) = &self.win_tx {
            let _ = tx.send(WinPowerCmd::Shutdown);
        }
    }
}

#[cfg(target_os = "windows")]
fn spawn_windows_power_thread() -> SyncSender<WinPowerCmd> {
    let (tx, rx) = mpsc::sync_channel::<WinPowerCmd>(4);
    std::thread::Builder::new()
        .name("catffeine-win-power".into())
        .spawn(move || {
            use windows_sys::Win32::System::Power::{
                SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
            };
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    WinPowerCmd::Prevent(reply) => {
                        // SAFETY: FFI call with valid execution state bitflags on pinned thread.
                        let prev = unsafe {
                            SetThreadExecutionState(
                                ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED,
                            )
                        };
                        let res = if prev == 0 {
                            Err("SetThreadExecutionState failed to prevent sleep".into())
                        } else {
                            Ok(())
                        };
                        let _ = reply.send(res);
                    }
                    WinPowerCmd::Allow(reply) => {
                        // SAFETY: Clearing execution flags on the same OS thread.
                        let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
                        let res = if prev == 0 {
                            Err("SetThreadExecutionState failed to restore sleep".into())
                        } else {
                            Ok(())
                        };
                        let _ = reply.send(res);
                    }
                    WinPowerCmd::Shutdown => {
                        unsafe {
                            SetThreadExecutionState(ES_CONTINUOUS);
                        }
                        break;
                    }
                }
            }
        })
        .expect("Failed to spawn Windows power management thread");
    tx
}

#[cfg(target_os = "linux")]
fn acquire_linux_inhibit() -> Result<LinuxInhibitHandle, String> {
    let connection = zbus::blocking::Connection::session()
        .map_err(|e| format!("Failed to connect to D-Bus session bus: {e}"))?;

    // Try standard Freedesktop ScreenSaver endpoints, then PowerManagement fallback.
    // Keep the Connection alive in LinuxInhibitHandle because D-Bus ScreenSaver
    // revokes the inhibition cookie if the requesting connection disconnects.
    let candidates: [(&str, &str, &str); 3] = [
        (
            "org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver",
        ),
        (
            "org.freedesktop.ScreenSaver",
            "/ScreenSaver",
            "org.freedesktop.ScreenSaver",
        ),
        (
            "org.freedesktop.PowerManagement",
            "/org/freedesktop/PowerManagement/Inhibit",
            "org.freedesktop.PowerManagement.Inhibit",
        ),
    ];

    let mut last_err = String::new();
    for (destination, path, interface) in candidates {
        match connection.call_method(
            Some(destination),
            path,
            Some(interface),
            "Inhibit",
            &("Catffeine", "Keeping system awake via Catffeine"),
        ) {
            Ok(reply) => {
                let cookie: u32 = reply
                    .body()
                    .deserialize()
                    .map_err(|e| format!("Failed to parse D-Bus Inhibit cookie: {e}"))?;
                return Ok(LinuxInhibitHandle {
                    connection,
                    destination,
                    path,
                    interface,
                    cookie,
                });
            }
            Err(err) => {
                last_err = format!("{destination} ({path}): {err}");
            }
        }
    }

    Err(format!(
        "Failed to acquire Freedesktop D-Bus sleep inhibition ({last_err})"
    ))
}

#[cfg(target_os = "linux")]
fn release_linux_inhibit(handle: LinuxInhibitHandle) -> Result<(), String> {
    handle
        .connection
        .call_method(
            Some(handle.destination),
            handle.path,
            Some(handle.interface),
            "UnInhibit",
            &(handle.cookie,),
        )
        .map_err(|e| format!("Failed to call D-Bus UnInhibit({}): {e}", handle.cookie))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_power_manager_tracks_prevent_and_allow_idempotently() {
        let mut pm = PowerManager::new_mock();
        assert!(!pm.is_preventing_sleep());

        pm.prevent_sleep().unwrap();
        assert!(pm.is_preventing_sleep());
        pm.prevent_sleep().unwrap();
        assert!(pm.is_preventing_sleep());

        pm.allow_sleep().unwrap();
        assert!(!pm.is_preventing_sleep());
        pm.allow_sleep().unwrap();
        assert!(!pm.is_preventing_sleep());
    }
}
