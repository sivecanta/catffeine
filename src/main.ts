import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type CatState = "sleeping" | "awake" | "working";
export type StayAwakePolicy = "off" | "indefinite" | "30m" | "1h" | "2h";
export type DockPosition = "bottom-right" | "bottom-left" | "pinned" | "free-drag";

export interface AppStateSnapshot {
  cat_state: CatState;
  policy: StayAwakePolicy;
  dock_position: DockPosition;
  always_on_top: boolean;
  cat_visible: boolean;
  active_agents: number;
  preventing_sleep: boolean;
  remaining_seconds: number | null;
  watcher_port: number;
}

const appRoot = document.getElementById("app") as HTMLElement;
const catHitbox = document.getElementById("cat-hitbox") as HTMLElement;
const statusBanner = document.getElementById("status-banner") as HTMLElement;
const statusLabel = document.getElementById("status-label") as HTMLElement;
const timerBadge = document.getElementById("timer-badge") as HTMLElement;
const quickPinBtn = document.getElementById("quick-pin-btn") as HTMLButtonElement;
const unhideBtn = document.getElementById("unhide-btn") as HTMLButtonElement;
const contextMenu = document.getElementById("context-menu") as HTMLElement;
const menuAlwaysOnTop = document.getElementById("menu-always-on-top") as HTMLButtonElement;
const menuToggleVisibleLabel = document.getElementById("menu-toggle-visible-label") as HTMLElement;

// Cache menu button collections once at startup
const policyButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-policy]"));
const positionButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-position]"));

let currentState: AppStateSnapshot = {
  cat_state: "sleeping",
  policy: "off",
  dock_position: "bottom-right",
  always_on_top: true,
  cat_visible: true,
  active_agents: 0,
  preventing_sleep: false,
  remaining_seconds: null,
  watcher_port: 41905,
};

let menuOpen = false;

export function formatDuration(seconds: number): string {
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hrs > 0) return `${hrs}h ${mins}m`;
  if (mins > 0) return `${mins}m ${secs}s`;
  return `${secs}s`;
}

function renderState(snapshot: AppStateSnapshot): void {
  currentState = snapshot;
  appRoot.dataset.state = snapshot.cat_state;
  appRoot.dataset.position = snapshot.dock_position;
  appRoot.dataset.visible = String(snapshot.cat_visible);

  // Toggle visibility between full cat view and compact "Show Cat" unhide pill
  catHitbox.classList.toggle("hidden", !snapshot.cat_visible);
  statusBanner.classList.toggle("hidden", !snapshot.cat_visible);
  unhideBtn.classList.toggle("hidden", snapshot.cat_visible);
  menuToggleVisibleLabel.textContent = snapshot.cat_visible ? "Hide Cat" : "Show Cat";

  // Show one-click "Pin Here" button whenever the cat is in free-drag mode
  quickPinBtn.classList.toggle("hidden", snapshot.dock_position !== "free-drag");

  if (snapshot.cat_state === "working") {
    const count = snapshot.active_agents;
    statusLabel.textContent = count === 1 ? "Working (1 agent)" : `Working (${count} agents)`;
    timerBadge.classList.add("hidden");
  } else if (snapshot.cat_state === "awake") {
    statusLabel.textContent = "Staying Awake";
    timerBadge.textContent =
      snapshot.remaining_seconds !== null && snapshot.remaining_seconds > 0
        ? formatDuration(snapshot.remaining_seconds)
        : "∞";
    timerBadge.classList.remove("hidden");
  } else {
    statusLabel.textContent = "Sleeping";
    timerBadge.classList.add("hidden");
  }

  for (const btn of policyButtons) {
    btn.classList.toggle("active", btn.dataset.policy === snapshot.policy);
  }
  for (const btn of positionButtons) {
    btn.classList.toggle("active", btn.dataset.position === snapshot.dock_position);
  }
  menuAlwaysOnTop.classList.toggle("active", snapshot.always_on_top);
}

function setMenuOpen(open: boolean): void {
  if (menuOpen === open) return;
  menuOpen = open;
  contextMenu.classList.toggle("hidden", !open);
}

async function toggleAwakePolicy(): Promise<void> {
  const policy: StayAwakePolicy = currentState.policy === "off" ? "indefinite" : "off";
  renderState(await invoke<AppStateSnapshot>("set_stay_awake_policy", { policy }));
}

// Left-click toggle & threshold-based free-drag on the cat
let pointerDownPos: { x: number; y: number } | null = null;
let dragStarted = false;

catHitbox.addEventListener("mousedown", (event) => {
  if (event.button !== 0) return;
  pointerDownPos = { x: event.screenX, y: event.screenY };
  dragStarted = false;
});

window.addEventListener("mousemove", async (event) => {
  if (!pointerDownPos || dragStarted) return;
  const moved =
    Math.abs(event.screenX - pointerDownPos.x) >= 4 ||
    Math.abs(event.screenY - pointerDownPos.y) >= 4;

  if (moved && currentState.dock_position === "free-drag") {
    dragStarted = true;
    pointerDownPos = null;
    try {
      await getCurrentWindow().startDragging();
    } catch {
      // Ignore in non-Tauri browser preview
    }
  }
});

catHitbox.addEventListener("mouseup", async (event) => {
  if (event.button !== 0 || !pointerDownPos) return;
  const moved =
    Math.abs(event.screenX - pointerDownPos.x) >= 4 ||
    Math.abs(event.screenY - pointerDownPos.y) >= 4;
  pointerDownPos = null;

  if (!moved && !dragStarted) {
    if (menuOpen) {
      setMenuOpen(false);
    } else {
      await toggleAwakePolicy();
    }
  }
});

catHitbox.addEventListener("keydown", async (event) => {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    await toggleAwakePolicy();
  }
});

// Quick "Pin Here" button locks the cat at its current custom coordinates
quickPinBtn.addEventListener("click", async () => {
  renderState(
    await invoke<AppStateSnapshot>("set_dock_position", {
      position: "pinned" as DockPosition,
    })
  );
});

// Quick "Show Cat" button restores the cat when hidden
unhideBtn.addEventListener("click", async () => {
  renderState(await invoke<AppStateSnapshot>("set_cat_visible", { visible: true }));
});

// Right-click & Escape context menu handling
window.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  setMenuOpen(!menuOpen);
});

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") setMenuOpen(false);
});

window.addEventListener("mousedown", (event) => {
  if (menuOpen && !(event.target instanceof Element && event.target.closest("#context-menu"))) {
    setMenuOpen(false);
  }
});

// Single delegated click listener for all context menu actions
contextMenu.addEventListener("click", async (event) => {
  const btn = event.target instanceof Element ? event.target.closest("button") : null;
  if (!btn) return;
  setMenuOpen(false);

  if (btn.dataset.policy) {
    renderState(
      await invoke<AppStateSnapshot>("set_stay_awake_policy", {
        policy: btn.dataset.policy as StayAwakePolicy,
      })
    );
  } else if (btn.dataset.position) {
    renderState(
      await invoke<AppStateSnapshot>("set_dock_position", {
        position: btn.dataset.position as DockPosition,
      })
    );
  } else if (btn.id === "menu-always-on-top") {
    renderState(
      await invoke<AppStateSnapshot>("set_always_on_top", {
        alwaysOnTop: !currentState.always_on_top,
      })
    );
  } else if (btn.id === "menu-toggle-visible") {
    renderState(
      await invoke<AppStateSnapshot>("set_cat_visible", {
        visible: !currentState.cat_visible,
      })
    );
  } else if (btn.id === "menu-quit") {
    await invoke("quit_app");
  }
});

async function init(): Promise<void> {
  try {
    await listen<AppStateSnapshot>("catffeine://state-changed", (event) => {
      renderState(event.payload);
    });
    renderState(await invoke<AppStateSnapshot>("get_app_state"));
  } catch (err) {
    console.warn("Running outside Tauri runtime or failed to fetch initial state:", err);
  }
}

void init();
