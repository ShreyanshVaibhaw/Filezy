/// Persisted user settings: one JSON object under one localStorage key, read
/// synchronously at module init so the theme is applied before the first paint.
/// Stored values are merged over DEFAULTS, so a key added in a later release
/// falls back to its default instead of breaking an existing settings blob.

export const DEFAULTS = {
  theme: "system",
  compact: false,
  showHidden: false,
  showExt: true,
  previewPane: false,
  // Where new tabs and the app itself start. A "::" page or a real folder path.
  startPage: "::Home",
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

const KEY = "settings";

function load(): Settings {
  let stored: Partial<Settings> = {};
  try {
    stored = JSON.parse(localStorage.getItem(KEY) ?? "{}");
  } catch {
    // Corrupt blob: boot on defaults rather than not booting.
  }
  const s: Settings = { ...structuredClone(DEFAULTS), ...stored };
  // Migration from the pre-settings keys.
  const oldTheme = localStorage.getItem("theme");
  const oldCompact = localStorage.getItem("compact");
  if (oldTheme !== null) s.theme = oldTheme;
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
  try {
    Object.assign(settings, JSON.parse(e.newValue));
    applyTheme();
  } catch {
    // ignore a torn write from the other window
  }
});

// ---- themes ----

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

function applyTheme() {
  if (settings.theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = settings.theme;
  syncWindowTheme();
}

/// The mica backdrop behind the title bar and nav pane is painted by Windows, not
/// by CSS, so a light palette on a dark-mode OS leaves unreadable chrome. Tell the
/// window which mode it is in; "system" hands the choice back to Windows.
/// The light/dark flag is read from the theme's own `color-scheme`, so themes.css
/// stays the single source of truth. Read inside the async continuation: at module
/// init the stylesheets are not applied yet.
async function syncWindowTheme() {
  const wanted = settings.theme;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
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
