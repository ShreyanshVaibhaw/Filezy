/// Persisted user settings: one JSON object under one localStorage key, read
/// synchronously at module init so the theme is applied before the first paint.
/// Stored values are checked against DEFAULTS (type, and allowed value for
/// choices), so a key added in a later release, or a damaged blob, falls back to
/// its default instead of breaking the app.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { VIEW_KEYS, DEFAULT_SORTS } from "./views";

export const DEFAULTS = {
  /** Bumped when stored settings need a migration step in load(). */
  schema: 1,
  theme: "system",
  compact: false,
  showHidden: false,
  // Explorer's "Hide protected operating system files" is on by default:
  // hidden+system items such as desktop.ini stay out of view even with
  // hidden items shown.
  showProtected: false,
  showExt: true,
  // Explorer's View > Show > Item check boxes.
  checkboxes: false,
  // Explorer's View > Show > Preview pane and Details pane (one at a time).
  previewPane: false,
  detailsPane: false,
  // Width of whichever side pane shows, in CSS pixels; dragged by its edge.
  paneWidth: 300,
  navWidth: 240,
  singleClick: false,
  // Explorer's Search options > Current folder / All subfolders.
  searchScope: "subfolders",
  // Recent searches, newest first (Search options > Recent searches).
  searchHistory: [] as string[],
  // Where new tabs and the app itself start. A "::" page or a real folder path.
  startPage: "::Home",
  // Reopen the last window's tabs when Filezy starts with nothing to open.
  // Off by default, like Explorer's "Restore previous folder windows at logon".
  restoreTabs: false,
  // Applied to folders with no saved view of their own.
  defaultView: "details",
  defaultSort: "name",
  defaultAsc: true,
  // Context-menu entries the user switched off, by id from menuitems.ts.
  // Only the exceptions are stored, so entries added later ship enabled.
  hiddenMenu: [] as string[],
  // Third-party shell extension entries in the item menu.
  shellExtras: true,
  // Extension entries switched off, and every one seen so far, by normalized
  // label: the shell's own item ids are rebuilt on every right-click.
  hiddenExtras: [] as string[],
  seenExtras: [] as string[],
};

export type Settings = typeof DEFAULTS;

export const THEMES: [string, string][] = [
  ["system", "System"],
  ["win11-dark", "Windows 11 Dark"],
  ["win11-light", "Windows 11 Light"],
  ["catppuccin-mocha", "Catppuccin Mocha"],
  ["catppuccin-latte", "Catppuccin Latte"],
  ["dracula", "Dracula"],
  ["nord", "Nord"],
  ["tokyo-night", "Tokyo Night"],
  ["gruvbox-dark", "Gruvbox Dark"],
];

const KEY = "settings";

// Allowed values for the settings that are a choice, so a stored value from a
// newer or hand-edited blob can't put the UI into a state it can't render.
const CHOICES: Partial<Record<keyof Settings, string[]>> = {
  defaultView: VIEW_KEYS,
  defaultSort: DEFAULT_SORTS.map(([k]) => k),
  searchScope: ["folder", "subfolders"],
};

function parse(json: string | null): Record<string, unknown> {
  try {
    const raw = JSON.parse(json ?? "{}");
    if (raw && typeof raw === "object" && !Array.isArray(raw)) return raw;
  } catch {
    // Corrupt blob: boot on defaults rather than not booting.
  }
  return {};
}

function sanitize(stored: Record<string, unknown>): Settings {
  const s: Settings = structuredClone(DEFAULTS);
  // Take each stored value only if it has the default's type (string arrays
  // for the lists) and, for choices, an allowed value.
  for (const k of Object.keys(DEFAULTS) as (keyof Settings)[]) {
    const v = stored[k];
    const d = DEFAULTS[k];
    const ok = Array.isArray(d)
      ? Array.isArray(v) && v.every((x) => typeof x === "string")
      : typeof v === typeof d && (!CHOICES[k] || CHOICES[k]!.includes(v as string));
    if (ok) (s as Record<string, unknown>)[k] = v;
  }
  if (!THEMES.some(([id]) => id === s.theme)) s.theme = DEFAULTS.theme;
  for (const [key, min, max] of [["paneWidth", 200, 600], ["navWidth", 160, 400]] as const) {
    s[key] = Number.isFinite(s[key]) ? Math.round(Math.min(max, Math.max(min, s[key]))) : DEFAULTS[key];
  }
  s.schema = DEFAULTS.schema;
  return s;
}

function load(): Settings {
  const s = sanitize(parse(localStorage.getItem(KEY)));
  // Migration from the pre-settings keys.
  const oldTheme = localStorage.getItem("theme");
  const oldCompact = localStorage.getItem("compact");
  if (oldTheme !== null && THEMES.some(([id]) => id === oldTheme)) s.theme = oldTheme;
  if (oldCompact !== null) s.compact = oldCompact === "1";
  if (oldTheme !== null || oldCompact !== null) {
    localStorage.removeItem("theme");
    localStorage.removeItem("compact");
    localStorage.setItem(KEY, JSON.stringify(s));
  }
  return s;
}

export const settings = $state<Settings>(load());

export function setSetting<K extends keyof Settings>(k: K, v: Settings[K]) {
  settings[k] = v;
  localStorage.setItem(KEY, JSON.stringify(settings));
}

export function resetAll() {
  Object.assign(settings, structuredClone(DEFAULTS));
  localStorage.setItem(KEY, JSON.stringify(settings));
  applyTheme();
}

// A second window (Ctrl+N) is another process sharing the same WebView2 profile,
// so it gets this event and follows along instead of drifting.
window.addEventListener("storage", (e) => {
  if (e.key !== KEY || !e.newValue) return;
  Object.assign(settings, sanitize(parse(e.newValue)));
  applyTheme();
});

// ---- themes ----


function applyTheme() {
  if (settings.theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = settings.theme;
  syncWindowTheme();
}

/// The mica backdrop behind the title bar and nav pane is painted by Windows, not
/// by CSS, so a light palette on a dark-mode OS leaves unreadable chrome. Tell the
/// window which mode it is in; "system" hands the choice back to Windows.
/// The light/dark flag is read from the theme's own `color-scheme`, so themes.css
/// stays the single source of truth. Read a frame later: at module init the
/// stylesheets are not applied yet.
async function syncWindowTheme() {
  const wanted = settings.theme;
  await new Promise((r) => requestAnimationFrame(r));
  if (settings.theme !== wanted) return; // a newer pick won
  const light = getComputedStyle(document.documentElement).colorScheme.includes("light");
  getCurrentWindow()
    .setTheme(wanted === "system" ? null : light ? "light" : "dark")
    .catch(() => {});
}

export function setTheme(id: string) {
  setSetting("theme", id);
  applyTheme();
}

// Apply the persisted theme once at startup.
applyTheme();

export function toggleCompact() {
  setSetting("compact", !settings.compact);
}

// ---- context menu entries ----

export function menuOn(id: string): boolean {
  return !settings.hiddenMenu.includes(id);
}

export function setMenuOn(id: string, on: boolean) {
  setSetting(
    "hiddenMenu",
    on ? settings.hiddenMenu.filter((x) => x !== id) : [...settings.hiddenMenu, id],
  );
}

/// True when at least one of these entries is still switched on, so a menu
/// with nothing left to show can decline to open at all.
export function anyMenuOn(ids: string[]): boolean {
  return ids.some(menuOn);
}

export function resetMenu() {
  setSetting("hiddenMenu", []);
}

// ---- shell extension entries ----
// They cannot be enumerated up front, so the list of switchable entries is
// learned from the menus the user actually opens.

const SEEN_CAP = 200;

/// "&Open with WinRAR..." and "Open with WinRAR" are the same entry.
export function normExtra(text: string): string {
  return text
    .replace(/&/g, "")
    .replace(/(\.\.\.|…)\s*$/, "")
    .trim();
}

export function extraOn(text: string): boolean {
  return !settings.hiddenExtras.includes(normExtra(text));
}

export function setExtraOn(name: string, on: boolean) {
  setSetting(
    "hiddenExtras",
    on ? settings.hiddenExtras.filter((x) => x !== name) : [...settings.hiddenExtras, name],
  );
}

export function rememberExtras(texts: string[]) {
  const known = new Set(settings.seenExtras);
  const add: string[] = [];
  for (const t of texts) {
    const n = normExtra(t);
    if (!n || known.has(n)) continue;
    known.add(n);
    add.push(n);
  }
  if (add.length) setSetting("seenExtras", [...settings.seenExtras, ...add].slice(-SEEN_CAP));
}

export function forgetExtras() {
  setSetting("seenExtras", []);
  setSetting("hiddenExtras", []);
}

/// Show the Preview pane, the Details pane, or neither: Explorer shows one at a time.
export function showPane(pane: "preview" | "details" | null) {
  setSetting("previewPane", pane === "preview");
  setSetting("detailsPane", pane === "details");
}
