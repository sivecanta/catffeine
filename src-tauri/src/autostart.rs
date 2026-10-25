//! Cross-platform Launch-on-Startup (Autostart) manager for Catffeine.
//!
//! Uses native OS mechanisms with zero external crate bloat:
//! - **Windows**: Copies the binary cleanly via raw byte stream to `%LOCALAPPDATA%\Catffeine\catffeine.exe`
//!   (avoiding WSL UNC paths at boot and Explorer Mark-of-the-Web attributes) and registers it in
//!   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (`Catffeine`).
//! - **macOS**: `~/Library/LaunchAgents/com.catffeine.desktop.plist` LaunchAgent.
//! - **Linux**: `${XDG_CONFIG_HOME:-~/.config}/autostart/catffeine.desktop` XDG autostart entry.

#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

/// Strips Windows extended-length path prefixes (`\\?\UNC\` or `\\?\`) for registry `Run` commands.
pub fn normalize_exe_path(raw: &str) -> String {
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        raw.to_string()
    }
}

/// Formats a macOS LaunchAgent `.plist` for starting Catffeine at login.
pub fn format_macos_plist(exe_path: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.catffeine.desktop</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_path}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#
    )
}

/// Formats an XDG `.desktop` autostart entry for Linux.
pub fn format_linux_desktop_entry(exe_path: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Catffeine\n\
         Comment=Cozy desktop companion cat and keep-awake utility\n\
         Exec=\"{exe_path}\"\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(not(target_os = "windows"))]
fn current_exe_string() -> Result<String, String> {
    let exe: PathBuf =
        std::env::current_exe().map_err(|e| format!("Failed to resolve current_exe: {e}"))?;
    Ok(normalize_exe_path(&exe.to_string_lossy()))
}

/// Checks whether Catffeine is currently configured to launch on OS startup.
pub fn is_launch_on_start_enabled() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_autostart::is_enabled()
    }
    #[cfg(target_os = "macos")]
    {
        macos_plist_path().is_some_and(|p| p.exists())
    }
    #[cfg(target_os = "linux")]
    {
        linux_autostart_path().is_some_and(|p| p.exists())
    }
}

/// Enables or disables launching Catffeine on OS startup.
pub fn set_launch_on_start(enabled: bool) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        if enabled {
            let exe = windows_autostart::prepare_local_autostart_exe()?;
            windows_autostart::enable(&exe)?;
        } else {
            windows_autostart::disable()?;
        }
        Ok(windows_autostart::is_enabled())
    }

    #[cfg(target_os = "macos")]
    {
        let path = macos_plist_path().ok_or("HOME directory not found")?;
        if enabled {
            let exe = current_exe_string()?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create LaunchAgents dir: {e}"))?;
            }
            std::fs::write(&path, format_macos_plist(&exe))
                .map_err(|e| format!("Failed to write LaunchAgent plist: {e}"))?;
        } else if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove LaunchAgent plist: {e}"))?;
        }
        Ok(path.exists())
    }

    #[cfg(target_os = "linux")]
    {
        let path = linux_autostart_path().ok_or("HOME / XDG_CONFIG_HOME not found")?;
        if enabled {
            let exe = current_exe_string()?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create autostart dir: {e}"))?;
            }
            std::fs::write(&path, format_linux_desktop_entry(&exe))
                .map_err(|e| format!("Failed to write autostart desktop file: {e}"))?;
        } else if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove autostart desktop file: {e}"))?;
        }
        Ok(path.exists())
    }
}

#[cfg(target_os = "macos")]
fn macos_plist_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join("Library")
            .join("LaunchAgents")
            .join("com.catffeine.desktop.plist")
    })
}

#[cfg(target_os = "linux")]
fn linux_autostart_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join("catffeine.desktop"))
}

#[cfg(target_os = "windows")]
mod windows_autostart {
    #[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
    };

    use super::normalize_exe_path;

    const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0";
    const VALUE_NAME: &str = "Catffeine\0";

    fn wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// Ensures the autostart executable lives on the local Windows NTFS drive
    /// (`%LOCALAPPDATA%\Catffeine\catffeine.exe`) written via raw byte stream
    /// so it works at Windows login before WSL boots and without Explorer MOTW tags.
    pub fn prepare_local_autostart_exe() -> Result<String, String> {
        let current =
            std::env::current_exe().map_err(|e| format!("Failed to resolve current_exe: {e}"))?;

        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let install_dir = PathBuf::from(local_app_data).join("Catffeine");
            let target_exe = install_dir.join("catffeine.exe");

            if current != target_exe && std::fs::create_dir_all(&install_dir).is_ok() {
                if let Ok(bytes) = std::fs::read(&current) {
                    let _ = std::fs::remove_file(&target_exe);
                    if std::fs::write(&target_exe, bytes).is_ok() {
                        return Ok(normalize_exe_path(&target_exe.to_string_lossy()));
                    }
                }
            }
            if target_exe.exists() {
                return Ok(normalize_exe_path(&target_exe.to_string_lossy()));
            }
        }

        Ok(normalize_exe_path(&current.to_string_lossy()))
    }

    pub fn is_enabled() -> bool {
        let subkey = wide_null(RUN_SUBKEY);
        let val_name = wide_null(VALUE_NAME);
        let mut hkey = std::ptr::null_mut();

        // SAFETY: Querying standard HKCU Run registry key with null-terminated wide strings.
        unsafe {
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut hkey,
            ) != ERROR_SUCCESS
            {
                return false;
            }
            let status = RegQueryValueExW(
                hkey,
                val_name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            RegCloseKey(hkey);
            status == ERROR_SUCCESS
        }
    }

    pub fn enable(exe_path: &str) -> Result<(), String> {
        let subkey = wide_null(RUN_SUBKEY);
        let val_name = wide_null(VALUE_NAME);
        let quoted = format!("\"{exe_path}\"\0");
        let data: Vec<u16> = quoted.encode_utf16().collect();
        let byte_len = (data.len() * 2) as u32;
        let mut hkey = std::ptr::null_mut();

        // SAFETY: Writing REG_SZ value to HKCU\...\Run and closing key handle.
        unsafe {
            let open_res = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            );
            if open_res != ERROR_SUCCESS {
                return Err(format!("RegOpenKeyExW failed with code {open_res}"));
            }
            let set_res = RegSetValueExW(
                hkey,
                val_name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                byte_len,
            );
            RegCloseKey(hkey);
            if set_res != ERROR_SUCCESS {
                return Err(format!("RegSetValueExW failed with code {set_res}"));
            }
        }
        Ok(())
    }

    pub fn disable() -> Result<(), String> {
        let subkey = wide_null(RUN_SUBKEY);
        let val_name = wide_null(VALUE_NAME);
        let mut hkey = std::ptr::null_mut();

        // SAFETY: Deleting Catffeine value from HKCU\...\Run if present.
        unsafe {
            let open_res = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            );
            if open_res != ERROR_SUCCESS {
                return Ok(());
            }
            let _ = RegDeleteValueW(hkey, val_name.as_ptr());
            RegCloseKey(hkey);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_windows_extended_length_paths() {
        assert_eq!(
            normalize_exe_path(r"\\?\C:\Users\kaizen\catffeine.exe"),
            r"C:\Users\kaizen\catffeine.exe"
        );
        assert_eq!(
            normalize_exe_path(r"\\?\UNC\wsl.localhost\Ubuntu\home\kaizen\catffeine.exe"),
            r"\\wsl.localhost\Ubuntu\home\kaizen\catffeine.exe"
        );
    }

    #[test]
    fn generates_valid_macos_and_linux_autostart_configs() {
        let plist = format_macos_plist("/Applications/Catffeine.app/Contents/MacOS/catffeine");
        assert!(plist.contains("<string>com.catffeine.desktop</string>"));
        assert!(plist.contains("<true/>"));

        let desktop = format_linux_desktop_entry("/usr/bin/catffeine");
        assert!(desktop.contains("Exec=\"/usr/bin/catffeine\""));
        assert!(desktop.contains("X-GNOME-Autostart-enabled=true"));
    }
}
