//! Window Positioning & Docking presets for Catffeine.
//!
//! Queries the active monitor's physical geometry and scale factor via Tauri
//! (`window.current_monitor()`) and positions the frameless window relative to
//! the desktop taskbar/dock.

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, WebviewWindow};
#[cfg(not(target_os = "windows"))]
use tauri::{PhysicalSize, Size};

/// Compact window height (logical px) for cat + status banner when context menu is closed.
pub const COMPACT_WINDOW_HEIGHT_LOGICAL: f64 = 196.0;
/// Expanded window height (logical px) to accommodate the open context menu.
pub const MENU_WINDOW_HEIGHT_LOGICAL: f64 = 480.0;

/// Taskbar / dock vertical clearance in logical pixels.
const TASKBAR_OFFSET_LOGICAL: f64 = 48.0;
/// Left-edge horizontal clearance in logical pixels.
const LEFT_EDGE_OFFSET_LOGICAL: f64 = 36.0;
/// Right-edge horizontal clearance (left of system tray) in logical pixels.
const RIGHT_TRAY_OFFSET_LOGICAL: f64 = 96.0;

/// Supported docking presets for the companion cat window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DockPosition {
    #[default]
    #[serde(rename = "bottom-right")]
    BottomRight,
    #[serde(rename = "bottom-left")]
    BottomLeft,
    #[serde(rename = "pinned")]
    Pinned,
    #[serde(rename = "free-drag")]
    FreeDrag,
}

/// Computes physical `(x, y)` coordinates for a docked window given monitor geometry.
/// Returns `None` for [`DockPosition::FreeDrag`] and [`DockPosition::Pinned`],
/// keeping the window at its current custom coordinates.
pub fn compute_dock_coords(
    position: DockPosition,
    mon_pos: (i32, i32),
    mon_size: (u32, u32),
    win_size: (u32, u32),
    scale: f64,
) -> Option<(i32, i32)> {
    if matches!(position, DockPosition::FreeDrag | DockPosition::Pinned) {
        return None;
    }

    let taskbar_offset = (TASKBAR_OFFSET_LOGICAL * scale).round() as i32;
    let target_y = mon_pos.1 + (mon_size.1 as i32) - (win_size.1 as i32) - taskbar_offset;

    let target_x = match position {
        DockPosition::BottomRight => {
            let tray_offset = (RIGHT_TRAY_OFFSET_LOGICAL * scale).round() as i32;
            mon_pos.0 + (mon_size.0 as i32) - (win_size.0 as i32) - tray_offset
        }
        DockPosition::BottomLeft => {
            let edge_offset = (LEFT_EDGE_OFFSET_LOGICAL * scale).round() as i32;
            mon_pos.0 + edge_offset
        }
        DockPosition::FreeDrag | DockPosition::Pinned => unreachable!(),
    };

    Some((target_x, target_y))
}

/// Applies the requested [`DockPosition`] to the given [`WebviewWindow`].
pub fn apply_dock_position(window: &WebviewWindow, position: DockPosition) -> Result<(), String> {
    if matches!(position, DockPosition::FreeDrag | DockPosition::Pinned) {
        return Ok(());
    }

    let monitor = window
        .current_monitor()
        .map_err(|e| format!("Failed to query current monitor: {e}"))?
        .or_else(|| window.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        return Ok(());
    };

    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let win_size = window
        .outer_size()
        .map_err(|e| format!("Failed to query window size: {e}"))?;

    if let Some((x, y)) = compute_dock_coords(
        position,
        (mon_pos.x, mon_pos.y),
        (mon_size.width, mon_size.height),
        (win_size.width, win_size.height),
        monitor.scale_factor(),
    ) {
        window
            .set_position(PhysicalPosition::new(x, y))
            .map_err(|e| format!("Failed to set window position: {e}"))?;
    }

    Ok(())
}

/// Computes a bottom-anchored `(x, y, width, height)` in physical pixels when resizing vertically
/// so the bottom edge (`y + height`) stays fixed on screen.
pub fn compute_bottom_anchored_rect(
    old_pos: (i32, i32),
    old_size: (u32, u32),
    target_logical_height: f64,
    scale: f64,
) -> (i32, i32, u32, u32) {
    let new_h = (target_logical_height * scale).round() as u32;
    let new_y = old_pos.1 + (old_size.1 as i32) - (new_h as i32);
    (old_pos.0, new_y, old_size.0, new_h)
}

/// Resizes the window vertically (anchored at its bottom edge) for open vs closed context menu.
pub fn set_window_menu_open_bounds(window: &WebviewWindow, menu_open: bool) -> Result<(), String> {
    let old_pos = window
        .outer_position()
        .map_err(|e| format!("Failed to query window position: {e}"))?;
    let old_size = window
        .outer_size()
        .map_err(|e| format!("Failed to query window size: {e}"))?;
    let scale = window
        .scale_factor()
        .map_err(|e| format!("Failed to query window scale factor: {e}"))?;

    let target_logical_height = if menu_open {
        MENU_WINDOW_HEIGHT_LOGICAL
    } else {
        COMPACT_WINDOW_HEIGHT_LOGICAL
    };

    let (x, y, w, h) = compute_bottom_anchored_rect(
        (old_pos.x, old_pos.y),
        (old_size.width, old_size.height),
        target_logical_height,
        scale,
    );

    if old_size.height == h {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
        };
        let hwnd = window
            .hwnd()
            .map_err(|e| format!("Failed to get HWND: {e}"))?;
        let ok = unsafe {
            SetWindowPos(
                hwnd.0,
                std::ptr::null_mut(),
                x,
                y,
                w as i32,
                h as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        if ok == 0 {
            return Err("SetWindowPos failed while resizing context menu bounds".to_string());
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        window
            .set_size(Size::Physical(PhysicalSize::new(w, h)))
            .map_err(|e| format!("Failed to set window size: {e}"))?;
        window
            .set_position(PhysicalPosition::new(x, y))
            .map_err(|e| format!("Failed to set window position: {e}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_drag_and_pinned_return_none_to_preserve_custom_coordinates() {
        assert_eq!(
            compute_dock_coords(DockPosition::FreeDrag, (0, 0), (1920, 1080), (220, 230), 1.0),
            None
        );
        assert_eq!(
            compute_dock_coords(DockPosition::Pinned, (0, 0), (1920, 1080), (220, 230), 1.0),
            None
        );
    }

    #[test]
    fn bottom_right_positions_above_taskbar_and_left_of_tray() {
        let coords = compute_dock_coords(
            DockPosition::BottomRight,
            (0, 0),
            (1920, 1080),
            (220, 230),
            1.0,
        );
        // x = 1920 - 220 - 96 = 1604, y = 1080 - 230 - 48 = 802
        assert_eq!(coords, Some((1604, 802)));
    }

    #[test]
    fn bottom_left_scales_offsets_on_hidpi_secondary_monitor() {
        let coords = compute_dock_coords(
            DockPosition::BottomLeft,
            (-1920, 100),
            (3840, 2160),
            (440, 460),
            2.0,
        );
        // x = -1920 + (36 * 2) = -1848, y = 100 + 2160 - 460 - (48 * 2) = 1704
        assert_eq!(coords, Some((-1848, 1704)));
    }

    #[test]
    fn bottom_anchored_rect_preserves_bottom_edge_across_menu_toggle() {
        let old_pos = (1604, 842);
        let old_size = (260, 196);
        let (x, y, w, h) =
            compute_bottom_anchored_rect(old_pos, old_size, MENU_WINDOW_HEIGHT_LOGICAL, 1.0);
        assert_eq!(x, old_pos.0);
        assert_eq!(w, old_size.0);
        assert_eq!(h, 480);
        assert_eq!(old_pos.1 + old_size.1 as i32, y + h as i32);

        // Scaled HiDPI restore back to compact height also preserves bottom edge
        let (x2, y2, w2, h2) =
            compute_bottom_anchored_rect((x, y * 2), (w * 2, h * 2), COMPACT_WINDOW_HEIGHT_LOGICAL, 2.0);
        assert_eq!(h2, 392);
        assert_eq!((y * 2) + (h * 2) as i32, y2 + h2 as i32);
        assert_eq!(x2, x);
        assert_eq!(w2, w * 2);
    }
}
