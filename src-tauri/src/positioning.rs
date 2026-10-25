//! Window Positioning & Docking presets for Catffeine.
//!
//! Queries the active monitor's physical geometry and scale factor via Tauri
//! (`window.current_monitor()`) and positions the frameless window relative to
//! the desktop taskbar/dock.

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, WebviewWindow};

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
}
