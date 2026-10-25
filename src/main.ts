import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type CatState = "sleeping" | "awake" | "working";
export type StayAwakePolicy = "off" | "indefinite" | "30m" | "1h" | "2h";
export type DockPosition = "bottom-right" | "bottom-left" | "pinned" | "free-drag";
export type TrackedAgent = "none" | "claude" | "codex" | "antigravity" | "cursor";

export interface AppStateSnapshot {
  cat_state: CatState;
  policy: StayAwakePolicy;
  dock_position: DockPosition;
  always_on_top: boolean;
  launch_on_start: boolean;
  cat_visible: boolean;
  tracked_agent: TrackedAgent;
  tracked_agent_busy: boolean;
  active_agents: number;
  preventing_sleep: boolean;
  remaining_seconds: number | null;
  watcher_port: number;
}

const appRoot = document.getElementById("app") as HTMLElement;
const catHitbox = document.getElementById("cat-hitbox") as HTMLElement;
const meowBubble = document.getElementById("meow-bubble") as HTMLElement;
const statusBanner = document.getElementById("status-banner") as HTMLElement;
const statusLabel = document.getElementById("status-label") as HTMLElement;
const timerBadge = document.getElementById("timer-badge") as HTMLElement;
const trackBtn = document.getElementById("track-btn") as HTMLButtonElement;
const trackMenu = document.getElementById("track-menu") as HTMLElement;
const quickPinBtn = document.getElementById("quick-pin-btn") as HTMLButtonElement;
const unhideBtn = document.getElementById("unhide-btn") as HTMLButtonElement;
const contextMenu = document.getElementById("context-menu") as HTMLElement;
const menuAlwaysOnTop = document.getElementById("menu-always-on-top") as HTMLButtonElement;
const menuLaunchOnStart = document.getElementById("menu-launch-on-start") as HTMLButtonElement;
const menuToggleVisibleLabel = document.getElementById("menu-toggle-visible-label") as HTMLElement;

// Cache menu button collections once at startup
const policyButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-policy]"));
const positionButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-position]"));
const trackButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-track]"));

let currentState: AppStateSnapshot = {
  cat_state: "sleeping",
  policy: "off",
  dock_position: "bottom-right",
  always_on_top: true,
  launch_on_start: false,
  cat_visible: true,
  tracked_agent: "none",
  tracked_agent_busy: false,
  active_agents: 0,
  preventing_sleep: false,
  remaining_seconds: null,
  watcher_port: 41905,
};

let menuOpen = false;
let audioCtx: AudioContext | null = null;
let meowTimer: number | null = null;

function playWakeMeow(text = "Meow!"): void {
  // Show pixel speech bubble
  meowBubble.textContent = text;
  meowBubble.classList.add("hidden");
  void meowBubble.offsetWidth; // restart animation
  meowBubble.classList.remove("hidden");
  if (meowTimer !== null) window.clearTimeout(meowTimer);
  meowTimer = window.setTimeout(() => {
    meowBubble.classList.add("hidden");
  }, 1400);

  // Synthesize a cozy pixel-cat "mew!" chirp via Web Audio API
  try {
    const Ctx = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    if (!audioCtx) audioCtx = new Ctx();
    if (audioCtx.state === "suspended") void audioCtx.resume();

    const now = audioCtx.currentTime;
    const osc = audioCtx.createOscillator();
    const gain = audioCtx.createGain();

    osc.type = "triangle";
    osc.frequency.setValueAtTime(520, now);
    osc.frequency.exponentialRampToValueAtTime(880, now + 0.09);
    osc.frequency.exponentialRampToValueAtTime(620, now + 0.24);

    gain.gain.setValueAtTime(0.001, now);
    gain.gain.linearRampToValueAtTime(0.18, now + 0.03);
    gain.gain.exponentialRampToValueAtTime(0.001, now + 0.26);

    osc.connect(gain);
    gain.connect(audioCtx.destination);
    osc.start(now);
    osc.stop(now + 0.27);
  } catch {
    // Ignore if audio device is unavailable
  }
}

export function formatDuration(seconds: number): string {
  const hrs = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hrs > 0) return `${hrs}h ${mins}m`;
  if (mins > 0) return `${mins}m ${secs}s`;
  return `${secs}s`;
}

const AGENT_NAMES: Record<TrackedAgent, string> = {
  none: "Track",
  claude: "Claude",
  codex: "Codex",
  antigravity: "Antigravity",
  cursor: "Cursor",
};

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

  // Update Track button label and active state
  trackBtn.textContent =
    snapshot.tracked_agent === "none" ? "Track" : `Track: ${AGENT_NAMES[snapshot.tracked_agent]}`;
  trackBtn.classList.toggle("active", snapshot.tracked_agent !== "none");
  for (const btn of trackButtons) {
    btn.classList.toggle("active", btn.dataset.track === snapshot.tracked_agent);
  }

  if (snapshot.cat_state === "working") {
    const count = snapshot.active_agents;
    statusLabel.textContent =
      snapshot.tracked_agent_busy && snapshot.tracked_agent !== "none"
        ? `Working (${AGENT_NAMES[snapshot.tracked_agent]})`
        : count === 1
          ? "Working (1 agent)"
          : `Working (${count} agents)`;
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
  menuLaunchOnStart.classList.toggle("active", snapshot.launch_on_start);
}

function setMenuOpen(open: boolean): void {
  if (menuOpen === open) return;
  menuOpen = open;
  contextMenu.classList.toggle("hidden", !open);
  if (open) {
    void getCurrentWindow().setFocus().catch(() => {});
  }
}

async function toggleAwakePolicy(): Promise<void> {
  const wasSleeping = currentState.cat_state === "sleeping";
  const policy: StayAwakePolicy = currentState.policy === "off" ? "indefinite" : "off";
  const next = await invoke<AppStateSnapshot>("set_stay_awake_policy", { policy });
  renderState(next);
  if (wasSleeping && next.cat_state !== "sleeping") {
    playWakeMeow();
  }
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

// Single-select Track Agent menu (clicking active agent unselects it)
trackMenu.addEventListener("click", async (event) => {
  const btn = event.target instanceof Element ? event.target.closest<HTMLButtonElement>("[data-track]") : null;
  if (!btn || !btn.dataset.track) return;
  const clicked = btn.dataset.track as TrackedAgent;
  const agent: TrackedAgent = currentState.tracked_agent === clicked ? "none" : clicked;
  renderState(await invoke<AppStateSnapshot>("set_tracked_agent", { agent }));
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

window.addEventListener("blur", () => {
  setMenuOpen(false);
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
    const wasSleeping = currentState.cat_state === "sleeping";
    const next = await invoke<AppStateSnapshot>("set_stay_awake_policy", {
      policy: btn.dataset.policy as StayAwakePolicy,
    });
    renderState(next);
    if (wasSleeping && next.cat_state !== "sleeping") {
      playWakeMeow();
    }
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
  } else if (btn.id === "menu-launch-on-start") {
    renderState(
      await invoke<AppStateSnapshot>("set_launch_on_start", {
        enabled: !currentState.launch_on_start,
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
    await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) setMenuOpen(false);
    });
    await listen<AppStateSnapshot>("catffeine://state-changed", (event) => {
      renderState(event.payload);
    });
    await listen<string>("catffeine://tracked-agent-done", (event) => {
      playWakeMeow(`${event.payload} done! Meow!`);
    });
    renderState(await invoke<AppStateSnapshot>("get_app_state"));
  } catch (err) {
    console.warn("Running outside Tauri runtime or failed to fetch initial state:", err);
  }
}

void init();
