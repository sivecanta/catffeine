pub mod autostart;
pub mod positioning;
pub mod power;
pub mod tracker;
pub mod watcher;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "linux"))]
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
#[cfg(not(target_os = "linux"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::autostart::{is_launch_on_start_enabled, set_launch_on_start as apply_os_autostart};
use crate::positioning::{apply_dock_position, DockPosition};
use crate::power::PowerManager;
use crate::tracker::{is_tracked_agent_busy, TrackedAgent};
use crate::watcher::{resolve_watcher_port, spawn_watcher_server};

/// Visual and behavioral state of the companion cat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatState {
    Sleeping,
    Awake,
    Working,
}

/// User-selected keep-awake policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StayAwakePolicy {
    #[default]
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "indefinite")]
    Indefinite,
    #[serde(rename = "until-agent-done")]
    UntilAgentDone,
    #[serde(rename = "30m")]
    Minutes30,
    #[serde(rename = "1h")]
    Hour1,
    #[serde(rename = "2h")]
    Hours2,
}

impl StayAwakePolicy {
    pub fn duration(self) -> Option<Duration> {
        match self {
            Self::Off | Self::Indefinite | Self::UntilAgentDone => None,
            Self::Minutes30 => Some(Duration::from_secs(30 * 60)),
            Self::Hour1 => Some(Duration::from_secs(60 * 60)),
            Self::Hours2 => Some(Duration::from_secs(2 * 60 * 60)),
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Serializable state snapshot sent to the frontend and HTTP `/status` endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppStateSnapshot {
    pub cat_state: CatState,
    pub policy: StayAwakePolicy,
    pub dock_position: DockPosition,
    pub always_on_top: bool,
    pub launch_on_start: bool,
    pub cat_visible: bool,
    pub tracked_agent: TrackedAgent,
    pub tracked_agent_busy: bool,
    pub active_agents: u32,
    pub preventing_sleep: bool,
    pub remaining_seconds: Option<u64>,
    pub watcher_port: u16,
}

/// Mutable internal application state protected by a `Mutex`.
pub struct AppStateInner {
    pub power: PowerManager,
    pub policy: StayAwakePolicy,
    pub policy_expires_at: Option<Instant>,
    pub dock_position: DockPosition,
    pub always_on_top: bool,
    pub launch_on_start: bool,
    pub cat_visible: bool,
    pub tracked_agent: TrackedAgent,
    pub tracked_agent_busy: bool,
    pub active_agents: u32,
    pub watcher_port: u16,
}

impl AppStateInner {
    pub fn new(watcher_port: u16) -> Self {
        let mut state = Self::with_power(PowerManager::new(), watcher_port);
        state.launch_on_start = is_launch_on_start_enabled();
        state
    }

    pub fn with_power(power: PowerManager, watcher_port: u16) -> Self {
        Self {
            power,
            policy: StayAwakePolicy::Off,
            policy_expires_at: None,
            dock_position: DockPosition::BottomRight,
            always_on_top: true,
            launch_on_start: false,
            cat_visible: true,
            tracked_agent: TrackedAgent::None,
            tracked_agent_busy: false,
            active_agents: 0,
            watcher_port,
        }
    }

    pub fn set_policy(&mut self, policy: StayAwakePolicy) {
        self.set_policy_at(policy, Instant::now());
    }

    pub fn set_policy_at(&mut self, policy: StayAwakePolicy, now: Instant) {
        self.policy = policy;
        self.policy_expires_at = policy.duration().map(|d| now + d);
    }

    pub fn agent_start(&mut self) {
        self.active_agents = self.active_agents.saturating_add(1);
    }

    pub fn agent_stop(&mut self) {
        let had_agents = self.active_agents > 0;
        self.active_agents = self.active_agents.saturating_sub(1);
        if had_agents
            && self.active_agents == 0
            && !self.tracked_agent_busy
            && self.policy == StayAwakePolicy::UntilAgentDone
        {
            self.set_policy(StayAwakePolicy::Off);
        }
    }

    pub fn set_tracked_agent_busy(&mut self, busy: bool) {
        let was_busy = self.tracked_agent_busy;
        self.tracked_agent_busy = busy;
        if was_busy
            && !busy
            && self.active_agents == 0
            && self.policy == StayAwakePolicy::UntilAgentDone
        {
            self.set_policy(StayAwakePolicy::Off);
        }
    }

    pub fn reset_for_quit(&mut self) {
        self.active_agents = 0;
        self.tracked_agent_busy = false;
        self.set_policy(StayAwakePolicy::Off);
    }

    /// Reconciles timed expiration, active agent count, and OS sleep inhibition at `Instant::now()`.
    pub fn reconcile(&mut self) -> AppStateSnapshot {
        self.reconcile_at(Instant::now())
    }

    /// Reconciles timed expiration, active agent count, and OS sleep inhibition at `now`.
    pub fn reconcile_at(&mut self, now: Instant) -> AppStateSnapshot {
        if let Some(deadline) = self.policy_expires_at {
            if now >= deadline {
                self.policy = StayAwakePolicy::Off;
                self.policy_expires_at = None;
            }
        }

        let total_agents = self.active_agents + u32::from(self.tracked_agent_busy);

        let cat_state = if total_agents > 0 {
            CatState::Working
        } else if self.policy.is_active() {
            CatState::Awake
        } else {
            CatState::Sleeping
        };

        let should_prevent_sleep = matches!(cat_state, CatState::Awake | CatState::Working);

        if should_prevent_sleep && !self.power.is_preventing_sleep() {
            if let Err(err) = self.power.prevent_sleep() {
                eprintln!("[catffeine] Warning: prevent_sleep failed: {err}");
            }
        } else if !should_prevent_sleep && self.power.is_preventing_sleep() {
            if let Err(err) = self.power.allow_sleep() {
                eprintln!("[catffeine] Warning: allow_sleep failed: {err}");
            }
        }

        let remaining_seconds = self
            .policy_expires_at
            .map(|deadline| deadline.saturating_duration_since(now).as_secs());

        AppStateSnapshot {
            cat_state,
            policy: self.policy,
            dock_position: self.dock_position,
            always_on_top: self.always_on_top,
            launch_on_start: self.launch_on_start,
            cat_visible: self.cat_visible,
            tracked_agent: self.tracked_agent,
            tracked_agent_busy: self.tracked_agent_busy,
            active_agents: total_agents,
            preventing_sleep: self.power.is_preventing_sleep(),
            remaining_seconds,
            watcher_port: self.watcher_port,
        }
    }
}

pub struct AppState(pub Mutex<AppStateInner>);

/// Mutates [`AppState`], reconciles OS power state, and broadcasts `catffeine://state-changed`.
pub fn sync_and_broadcast_state<F>(app: &AppHandle, mutate: F) -> AppStateSnapshot
where
    F: FnOnce(&mut AppStateInner),
{
    let state = app.state::<AppState>();
    let snapshot = {
        let mut inner = state.0.lock().expect("AppState mutex poisoned");
        mutate(&mut inner);
        inner.reconcile()
    };

    let _ = app.emit("catffeine://state-changed", &snapshot);
    snapshot
}

fn apply_position_and_sync(
    app: &AppHandle,
    window: Option<&WebviewWindow>,
    position: DockPosition,
) -> Result<AppStateSnapshot, String> {
    if let Some(win) = window {
        apply_dock_position(win, position)?;
    } else if let Some(win) = app.get_webview_window("main") {
        apply_dock_position(&win, position)?;
    }
    Ok(sync_and_broadcast_state(app, |inner| {
        inner.dock_position = position;
    }))
}

fn apply_always_on_top_and_sync(
    app: &AppHandle,
    window: Option<&WebviewWindow>,
    always_on_top: bool,
) -> Result<AppStateSnapshot, String> {
    if let Some(win) = window {
        win.set_always_on_top(always_on_top)
            .map_err(|e| format!("Failed to toggle always_on_top: {e}"))?;
    } else if let Some(win) = app.get_webview_window("main") {
        win.set_always_on_top(always_on_top)
            .map_err(|e| format!("Failed to toggle always_on_top: {e}"))?;
    }
    Ok(sync_and_broadcast_state(app, |inner| {
        inner.always_on_top = always_on_top;
    }))
}

pub fn apply_cat_visible_and_sync(
    app: &AppHandle,
    window: Option<&WebviewWindow>,
    visible: bool,
) -> AppStateSnapshot {
    let win = window
        .cloned()
        .or_else(|| app.get_webview_window("main"));

    if let Some(win) = win {
        if visible {
            let _ = win.show();
            let _ = win.set_always_on_top(true);
            let _ = win.set_focus();
        } else {
            // On Windows/macOS where the system tray icon is present, hide the window into the tray;
            // on Linux/WSL without tray, keep the window visible so the in-window "Show Cat" pill works.
            #[cfg(not(target_os = "linux"))]
            let _ = win.hide();
        }
    }

    sync_and_broadcast_state(app, |inner| {
        inner.cat_visible = visible;
        if visible {
            inner.always_on_top = true;
        }
    })
}

fn apply_launch_on_start_and_sync(
    app: &AppHandle,
    enabled: bool,
) -> Result<AppStateSnapshot, String> {
    let actual = apply_os_autostart(enabled)?;
    Ok(sync_and_broadcast_state(app, |inner| {
        inner.launch_on_start = actual;
    }))
}

fn shutdown_and_exit(app: &AppHandle) {
    let _ = sync_and_broadcast_state(app, AppStateInner::reset_for_quit);
    app.exit(0);
}

#[tauri::command]
fn get_app_state(app: AppHandle) -> AppStateSnapshot {
    sync_and_broadcast_state(&app, |_| {})
}

#[tauri::command]
fn set_stay_awake_policy(app: AppHandle, policy: StayAwakePolicy) -> AppStateSnapshot {
    sync_and_broadcast_state(&app, |inner| inner.set_policy(policy))
}

#[tauri::command]
fn set_dock_position(
    app: AppHandle,
    window: WebviewWindow,
    position: DockPosition,
) -> Result<AppStateSnapshot, String> {
    apply_position_and_sync(&app, Some(&window), position)
}

#[tauri::command]
fn set_always_on_top(
    app: AppHandle,
    window: WebviewWindow,
    always_on_top: bool,
) -> Result<AppStateSnapshot, String> {
    apply_always_on_top_and_sync(&app, Some(&window), always_on_top)
}

#[tauri::command]
fn set_launch_on_start(app: AppHandle, enabled: bool) -> Result<AppStateSnapshot, String> {
    apply_launch_on_start_and_sync(&app, enabled)
}

#[tauri::command]
fn set_cat_visible(app: AppHandle, window: WebviewWindow, visible: bool) -> AppStateSnapshot {
    apply_cat_visible_and_sync(&app, Some(&window), visible)
}

#[tauri::command]
fn set_tracked_agent(app: AppHandle, agent: TrackedAgent) -> AppStateSnapshot {
    let busy = is_tracked_agent_busy(agent);
    sync_and_broadcast_state(&app, |inner| {
        inner.tracked_agent = agent;
        inner.tracked_agent_busy = busy;
    })
}

#[tauri::command]
fn set_ignore_cursor_events(window: WebviewWindow, ignore: bool) -> Result<(), String> {
    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| format!("Failed to set ignore cursor events: {e}"))
}

#[tauri::command]
fn agent_start(app: AppHandle) -> AppStateSnapshot {
    sync_and_broadcast_state(&app, AppStateInner::agent_start)
}

#[tauri::command]
fn agent_stop(app: AppHandle) -> AppStateSnapshot {
    sync_and_broadcast_state(&app, AppStateInner::agent_stop)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    shutdown_and_exit(&app);
}

#[cfg(not(target_os = "linux"))]
fn setup_system_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let stay_awake_submenu = SubmenuBuilder::new(app, "Stay Awake")
        .item(&MenuItemBuilder::with_id("policy_indefinite", "Indefinite").build(app)?)
        .item(&MenuItemBuilder::with_id("policy_until_agent_done", "Until Agent Done").build(app)?)
        .item(&MenuItemBuilder::with_id("policy_30m", "30 Minutes").build(app)?)
        .item(&MenuItemBuilder::with_id("policy_1h", "1 Hour").build(app)?)
        .item(&MenuItemBuilder::with_id("policy_2h", "2 Hours").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("policy_off", "Off (Allow Sleep)").build(app)?)
        .build()?;

    let position_submenu = SubmenuBuilder::new(app, "Position")
        .item(&MenuItemBuilder::with_id("pos_pinned", "Pin Here (Lock Position)").build(app)?)
        .item(&MenuItemBuilder::with_id("pos_free_drag", "Unpin & Move (Free Drag)").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("pos_bottom_right", "Bottom Right").build(app)?)
        .item(&MenuItemBuilder::with_id("pos_bottom_left", "Bottom Left").build(app)?)
        .build()?;

    let toggle_visible =
        MenuItemBuilder::with_id("toggle_visible", "Show / Hide Cat").build(app)?;
    let toggle_aot =
        MenuItemBuilder::with_id("toggle_always_on_top", "Toggle Always on Top").build(app)?;
    let toggle_autostart =
        MenuItemBuilder::with_id("toggle_launch_on_start", "Toggle Launch on Startup").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit Catffeine").build(app)?;

    let tray_menu = MenuBuilder::new(app)
        .item(&toggle_visible)
        .separator()
        .item(&stay_awake_submenu)
        .item(&position_submenu)
        .separator()
        .item(&toggle_aot)
        .item(&toggle_autostart)
        .separator()
        .item(&quit_item)
        .build()?;

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .tooltip("Catffeine — Left-click to Show/Hide Cat | Right-click for Menu")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                let next = !app
                    .state::<AppState>()
                    .0
                    .lock()
                    .expect("AppState mutex poisoned")
                    .cat_visible;
                apply_cat_visible_and_sync(app, None, next);
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle_visible" => {
                let next = !app
                    .state::<AppState>()
                    .0
                    .lock()
                    .expect("AppState mutex poisoned")
                    .cat_visible;
                apply_cat_visible_and_sync(app, None, next);
            }
            "policy_indefinite" => {
                sync_and_broadcast_state(app, |s| s.set_policy(StayAwakePolicy::Indefinite));
            }
            "policy_until_agent_done" => {
                sync_and_broadcast_state(app, |s| s.set_policy(StayAwakePolicy::UntilAgentDone));
            }
            "policy_30m" => {
                sync_and_broadcast_state(app, |s| s.set_policy(StayAwakePolicy::Minutes30));
            }
            "policy_1h" => {
                sync_and_broadcast_state(app, |s| s.set_policy(StayAwakePolicy::Hour1));
            }
            "policy_2h" => {
                sync_and_broadcast_state(app, |s| s.set_policy(StayAwakePolicy::Hours2));
            }
            "policy_off" => {
                sync_and_broadcast_state(app, |s| s.set_policy(StayAwakePolicy::Off));
            }
            "pos_bottom_left" => {
                let _ = apply_position_and_sync(app, None, DockPosition::BottomLeft);
            }
            "pos_bottom_right" => {
                let _ = apply_position_and_sync(app, None, DockPosition::BottomRight);
            }
            "pos_pinned" => {
                let _ = apply_position_and_sync(app, None, DockPosition::Pinned);
            }
            "pos_free_drag" => {
                let _ = apply_position_and_sync(app, None, DockPosition::FreeDrag);
            }
            "toggle_always_on_top" => {
                let next = !app
                    .state::<AppState>()
                    .0
                    .lock()
                    .expect("AppState mutex poisoned")
                    .always_on_top;
                let _ = apply_always_on_top_and_sync(app, None, next);
            }
            "toggle_launch_on_start" => {
                let next = !app
                    .state::<AppState>()
                    .0
                    .lock()
                    .expect("AppState mutex poisoned")
                    .launch_on_start;
                let _ = apply_launch_on_start_and_sync(app, next);
            }
            "quit" => shutdown_and_exit(app),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    tray_builder.build(app)?;
    Ok(())
}

fn spawn_timer_worker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let tracked = {
                let state = app.state::<AppState>();
                let inner = state.0.lock().expect("AppState mutex poisoned");
                inner.tracked_agent
            };

            let next_busy = if tracked != TrackedAgent::None {
                is_tracked_agent_busy(tracked)
            } else {
                false
            };

            let maybe_events = {
                let state = app.state::<AppState>();
                let mut inner = state.0.lock().expect("AppState mutex poisoned");
                let prev_busy = inner.tracked_agent_busy;
                let busy_changed = prev_busy != next_busy;
                if busy_changed {
                    inner.set_tracked_agent_busy(next_busy);
                }
                let finished_agent = if prev_busy && !next_busy && inner.tracked_agent != TrackedAgent::None {
                    Some(inner.tracked_agent.display_name().to_string())
                } else {
                    None
                };

                if busy_changed || inner.policy_expires_at.is_some() {
                    Some((inner.reconcile(), finished_agent))
                } else {
                    None
                }
            };

            if let Some((snapshot, finished_agent)) = maybe_events {
                let _ = app.emit("catffeine://state-changed", &snapshot);
                if let Some(agent_name) = finished_agent {
                    let _ = app.emit("catffeine://tracked-agent-done", &agent_name);
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let watcher_port = resolve_watcher_port();

    tauri::Builder::default()
        .manage(AppState(Mutex::new(AppStateInner::new(watcher_port))))
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();

            if let Some(main_window) = app.get_webview_window("main") {
                let _ = main_window.set_always_on_top(true);
                let _ = apply_dock_position(&main_window, DockPosition::BottomRight);
            }

            #[cfg(not(target_os = "linux"))]
            if let Err(err) = setup_system_tray(&handle) {
                eprintln!("[catffeine] System tray initialization skipped: {err}");
            }

            spawn_watcher_server(handle.clone(), watcher_port);
            spawn_timer_worker(handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            set_stay_awake_policy,
            set_dock_position,
            set_always_on_top,
            set_launch_on_start,
            set_cat_visible,
            set_tracked_agent,
            set_ignore_cursor_events,
            agent_start,
            agent_stop,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running Catffeine Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppStateInner {
        AppStateInner::with_power(PowerManager::new_mock(), 41905)
    }

    #[test]
    fn default_state_is_sleeping_visible_and_always_on_top() {
        let mut state = test_state();
        let snap = state.reconcile();
        assert_eq!(snap.cat_state, CatState::Sleeping);
        assert_eq!(snap.policy, StayAwakePolicy::Off);
        assert!(snap.always_on_top);
        assert!(snap.cat_visible);
        assert!(!snap.preventing_sleep);
        assert_eq!(snap.active_agents, 0);
        assert_eq!(snap.remaining_seconds, None);
    }

    #[test]
    fn timed_policy_prevents_sleep_until_deadline_expires() {
        let mut state = test_state();
        let start = Instant::now();

        state.set_policy_at(StayAwakePolicy::Minutes30, start);
        let snap = state.reconcile_at(start + Duration::from_secs(600));
        assert_eq!(snap.cat_state, CatState::Awake);
        assert_eq!(snap.policy, StayAwakePolicy::Minutes30);
        assert!(snap.preventing_sleep);
        assert_eq!(snap.remaining_seconds, Some(1200));

        // At exact 30m deadline, policy resets to Off and sleep is allowed again
        let expired = state.reconcile_at(start + Duration::from_secs(1800));
        assert_eq!(expired.cat_state, CatState::Sleeping);
        assert_eq!(expired.policy, StayAwakePolicy::Off);
        assert!(!expired.preventing_sleep);
        assert_eq!(expired.remaining_seconds, None);
    }

    #[test]
    fn active_agents_override_policy_to_working_and_restore_on_stop() {
        let mut state = test_state();

        state.agent_start();
        let snap = state.reconcile();
        assert_eq!(snap.cat_state, CatState::Working);
        assert_eq!(snap.active_agents, 1);
        assert!(snap.preventing_sleep);

        // Starting a second agent while user also enables Indefinite policy
        state.agent_start();
        state.set_policy(StayAwakePolicy::Indefinite);
        let snap = state.reconcile();
        assert_eq!(snap.cat_state, CatState::Working);
        assert_eq!(snap.active_agents, 2);

        // Stopping both agents restores the user's Indefinite Awake policy
        state.agent_stop();
        state.agent_stop();
        let snap = state.reconcile();
        assert_eq!(snap.cat_state, CatState::Awake);
        assert_eq!(snap.active_agents, 0);
        assert!(snap.preventing_sleep);

        // Extra stop calls saturate at 0 without underflow
        state.agent_stop();
        assert_eq!(state.active_agents, 0);
    }

    #[test]
    fn until_agent_done_policy_sleeps_when_tracked_agent_finishes() {
        let mut state = test_state();
        state.set_policy(StayAwakePolicy::UntilAgentDone);

        // Stays awake while waiting for an agent to start
        let snap = state.reconcile();
        assert_eq!(snap.cat_state, CatState::Awake);
        assert!(snap.preventing_sleep);

        // Enters working when tracked agent becomes busy
        state.set_tracked_agent_busy(true);
        let snap = state.reconcile();
        assert_eq!(snap.cat_state, CatState::Working);
        assert!(snap.preventing_sleep);

        // Automatically resets to Off / Sleeping when tracked agent finishes
        state.set_tracked_agent_busy(false);
        let snap = state.reconcile();
        assert_eq!(snap.policy, StayAwakePolicy::Off);
        assert_eq!(snap.cat_state, CatState::Sleeping);
        assert!(!snap.preventing_sleep);
    }
}
