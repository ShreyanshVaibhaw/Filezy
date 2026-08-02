import { invoke } from "@tauri-apps/api/core";
import { SvelteMap, SvelteSet } from "svelte/reactivity";

export interface Entry {
  name: string;
  is_dir: boolean;
  size: number;
  modified: number;
  created: number;
  ext: string;
  hidden: boolean;
  parent?: string; // set on search results
}
export interface ExtMeta {
  icon: string | null;
  type_name: string;
}
export interface Drive {
  path: string;
  name: string;
  total: number;
  free: number;
}

export interface Tab {
  id: number;
  path: string;
  hist: string[];
  histIdx: number;
  scroll: number;
}
export interface Group {
  label: string;
  start: number;
  count: number;
}

export const app = $state({
  path: "",
  home: "",
  total: 0,
  version: 0,
  sortKey: "name",
  asc: true,
  view: "details",
  previewPane: false,
  // ponytail: session-only view toggles; persist in a settings file if missed
  showHidden: false,
  showExt: true,
  theme: "system",
  compact: localStorage.getItem("compact") === "1",
  filter: "",
  searching: false,
  groupKey: "none",
  groups: [] as Group[],
  histIdx: -1,
  histLen: 0,
  activeTab: 0,
  canPaste: false,
  renaming: -1,
  anchor: 0,
  // ponytail: select-all is a flag, not 100k Set inserts; per-item ops resolve through selIndices()
  allSelected: false,
  status: "",
  drives: [] as Drive[],
  specials: [] as [string, string][],
  pins: [] as string[],
  onedrive: "",
});

export const tabs = $state<Tab[]>([]);
let nextTabId = 1;
function activeTab(): Tab | undefined {
  return tabs.find((t) => t.id === app.activeTab);
}
function syncNav() {
  const t = activeTab();
  app.histIdx = t?.histIdx ?? -1;
  app.histLen = t?.hist.length ?? 0;
}

export const sel = new SvelteSet<number>();
export const collapsedGroups = new SvelteSet<string>();

export function clearSel() {
  sel.clear();
  app.allSelected = false;
}
export function isSelected(i: number): boolean {
  return app.allSelected || sel.has(i);
}
export function selCount(): number {
  return app.allSelected ? app.total : sel.size;
}
export function selIndices(): number[] {
  return app.allSelected ? Array.from({ length: app.total }, (_, i) => i) : [...sel];
}
// Sparse cache of listing rows; holes are unfetched. Never holds more than what was scrolled past.
export const rows = $state<(Entry | undefined)[]>([]);
export const extMeta = new SvelteMap<string, ExtMeta>();
export const pathIcons = new SvelteMap<string, string | null>();

// Thumbnails for icon/tile views, keyed "path|size".
export const thumbs = new SvelteMap<string, string | null>();

// Real shell icons for fixed chrome (nav pane, Home tiles, drives), keyed "path|size".
export const shellIcons = new SvelteMap<string, string | null>();
export function shellIcon(path: string, size: number): string | null {
  const key = path + "|" + size;
  const v = shellIcons.get(key);
  if (v === undefined) {
    // Callable from templates: mutating reactive state during render is illegal
    // in Svelte 5, so the fetch kicks off in a microtask.
    queueMicrotask(() => {
      if (shellIcons.has(key)) return;
      shellIcons.set(key, null);
      invoke<string | null>("path_thumb", { path, size }).then((ic) => {
        if (ic) shellIcons.set(key, ic);
      });
    });
  }
  return v ?? null;
}

// Callbacks registered by FilePane so store actions can control the scroller.
export const hooks: {
  scrollTo?: (px: number) => void;
  scrollToIndex?: (i: number) => void;
  getScroll?: () => number;
  gridCols?: number;
  hStep?: number;
} = {};

const CHUNK = 256;
const pending = new Set<string>();

// ---- per-folder view/sort preferences (Explorer folder view settings) ----

interface FolderPrefs {
  s: string;
  a: boolean;
  v: string;
  g: string;
}
const DEFAULT_PREFS: FolderPrefs = { s: "name", a: true, v: "details", g: "none" };
let folderPrefs: Record<string, FolderPrefs> = JSON.parse(
  localStorage.getItem("folderPrefs") ?? "{}",
);
// True while the current grouping came from date-sorting, not a user choice.
let groupAuto = false;

function prefKey(p: string): string {
  return p.replace(/\\+$/, "").toLowerCase();
}

function savePrefs() {
  if (!app.path || app.path.startsWith("::")) return;
  // Auto date-grouping is derived from the sort, so it is stored as "none".
  folderPrefs[prefKey(app.path)] = {
    s: app.sortKey,
    a: app.asc,
    v: app.view,
    g: groupAuto ? "none" : app.groupKey,
  };
  const keys = Object.keys(folderPrefs);
  // ponytail: drop the oldest insertion when the map grows unbounded
  if (keys.length > 500) delete folderPrefs[keys[0]];
  localStorage.setItem("folderPrefs", JSON.stringify(folderPrefs));
}

function isDateKey(k: string): boolean {
  return k === "modified" || k === "created";
}

/// Apply a folder's saved prefs (or the fixed defaults) to the app state.
/// Returns without touching the backend; callers list/group afterwards.
function applyPrefs(p: string) {
  const prefs = folderPrefs[prefKey(p)] ?? DEFAULT_PREFS;
  app.sortKey = prefs.s;
  app.asc = prefs.a;
  app.view = prefs.v;
  groupAuto = false;
  app.groupKey = prefs.g;
  // Explorer behavior: date-sorted folders group by date automatically.
  if (prefs.g === "none" && isDateKey(prefs.s)) {
    app.groupKey = prefs.s;
    groupAuto = true;
  }
}

export function setView(v: string) {
  app.view = v;
  savePrefs();
}

export function toggleCompact() {
  app.compact = !app.compact;
  localStorage.setItem("compact", app.compact ? "1" : "0");
}

// Reopen-closed-tab stack (Ctrl+Shift+T), session-only.
const closedTabs: string[] = [];
export function reopenClosedTab() {
  const p = closedTabs.pop();
  if (p) newTab(p);
}

// Virtual locations (no backend listing).
export const THIS_PC = "::ThisPC";
export const HOME = "::Home";
export const RECYCLE = "::RecycleBin";
// Shell GUID of the bin itself, for its real icon.
export const RECYCLE_GUID = "::{645FF040-5081-101B-9F08-00AA002F954E}";

export interface RecycleItem {
  name: string;
  original: string;
  deleted: string;
  size: string;
}
export const recycleItems = $state<{ list: RecycleItem[] }>({ list: [] });

export interface RecentFile {
  name: string;
  path: string;
  ext: string;
  modified: number;
}
export const recentFiles = $state<{ list: RecentFile[] }>({ list: [] });

export function folderName(p: string): string {
  if (p === THIS_PC) return "This PC";
  if (p === HOME) return "Home";
  if (p === RECYCLE) return "Recycle Bin";
  const t = p.replace(/\\$/, "");
  const i = t.lastIndexOf("\\");
  return i < 0 ? p : t.slice(i + 1) || p;
}

export function joinPath(dir: string, name: string): string {
  return dir.replace(/\\$/, "") + "\\" + name;
}

export function entryPath(e: Entry): string {
  return joinPath(e.parent ?? app.path, e.name);
}

/// Name as shown in the panes: extension stripped when "File name extensions" is off.
export function displayName(e: Entry): string {
  if (app.showExt || e.is_dir || !e.ext) return e.name;
  const cut = e.name.length - e.ext.length - 1;
  return cut > 0 ? e.name.slice(0, cut) : e.name;
}

export function toggleHidden() {
  app.showHidden = !app.showHidden;
  refresh();
}

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

export function setTheme(id: string) {
  app.theme = id;
  if (id === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = id;
  localStorage.setItem("theme", id);
}

// Apply the persisted theme once at startup.
setTheme(localStorage.getItem("theme") ?? "system");

// ---- Quick Access pins ----

export async function loadPins() {
  app.pins = await invoke<string[]>("get_pins").catch(() => []);
}
export function isPinned(path: string): boolean {
  const n = path.replace(/\\$/, "").toLowerCase();
  return app.pins.some((p) => p.replace(/\\$/, "").toLowerCase() === n);
}
export async function pinAdd(path: string) {
  if (isPinned(path)) return;
  app.pins = [...app.pins, path];
  await invoke("set_pins", { pins: app.pins }).catch((e) => (app.status = String(e)));
}
export async function pinRemove(path: string) {
  const n = path.replace(/\\$/, "").toLowerCase();
  app.pins = app.pins.filter((p) => p.replace(/\\$/, "").toLowerCase() !== n);
  await invoke("set_pins", { pins: app.pins }).catch((e) => (app.status = String(e)));
}

function resetRows(total: number) {
  app.total = total;
  app.version++;
  rows.length = 0;
  rows.length = total;
  clearSel();
  app.renaming = -1;
  app.anchor = 0;
}

function loadVirtual(p: string) {
  if (p === THIS_PC) invoke<typeof app.drives>("drives").then((d) => (app.drives = d));
  if (p === HOME) invoke<RecentFile[]>("recent_files").then((r) => (recentFiles.list = r));
  if (p === RECYCLE) {
    invoke<RecycleItem[]>("recycle_list")
      .then((r) => (recycleItems.list = r))
      .catch((e) => (app.status = String(e)));
  }
}

export async function navigate(p: string, push = true) {
  if (p === THIS_PC || p === HOME || p === RECYCLE) {
    const t = activeTab();
    if (t) {
      if (push) {
        t.hist.splice(t.histIdx + 1);
        t.hist.push(p);
        t.histIdx = t.hist.length - 1;
      }
      t.path = p;
      syncNav();
    }
    app.path = p;
    app.filter = "";
    app.searching = false;
    app.status = "";
    resetRows(0);
    loadVirtual(p);
    refreshClipState();
    return;
  }
  try {
    applyPrefs(p);
    const res = await invoke<{ path: string; total: number }>("open_dir", {
      path: p,
      sortKey: app.sortKey,
      asc: app.asc,
      showHidden: app.showHidden,
    });
    const t = activeTab();
    if (t) {
      if (push) {
        t.hist.splice(t.histIdx + 1);
        t.hist.push(res.path);
        t.histIdx = t.hist.length - 1;
      }
      t.path = res.path;
      syncNav();
    }
    app.path = res.path;
    app.filter = "";
    app.searching = false; // navigating cancels any in-flight search (backend gen bump)
    app.status = "";
    resetRows(res.total);
    await reapplyGroup();
    hooks.scrollTo?.(0);
    refreshClipState();
  } catch (e) {
    app.status = String(e);
  }
}

export async function refresh(keepScroll = true) {
  if (!app.path) return;
  if (app.path.startsWith("::")) {
    loadVirtual(app.path);
    return;
  }
  if (app.searching) return; // don't clobber streaming search results
  const res = await invoke<{ path: string; total: number }>("open_dir", {
    path: app.path,
    sortKey: app.sortKey,
    asc: app.asc,
    showHidden: app.showHidden,
  });
  app.filter = "";
  resetRows(res.total);
  await reapplyGroup();
  if (!keepScroll) hooks.scrollTo?.(0);
}

export function back() {
  const t = activeTab();
  if (t && t.histIdx > 0) {
    t.histIdx--;
    syncNav();
    navigate(t.hist[t.histIdx], false);
  }
}
export function forward() {
  const t = activeTab();
  if (t && t.histIdx < t.hist.length - 1) {
    t.histIdx++;
    syncNav();
    navigate(t.hist[t.histIdx], false);
  }
}

function saveScroll() {
  const t = activeTab();
  if (t) t.scroll = hooks.getScroll?.() ?? 0;
}

// The virtualized pane needs a frame to rebuild its spacer height after a
// re-list; restoring scrollTop before that just clamps to 0.
function restoreScroll(px: number) {
  setTimeout(() => hooks.scrollTo?.(px), 0);
}

export function newTab(path?: string) {
  saveScroll();
  const id = nextTabId++;
  tabs.push({ id, path: "", hist: [], histIdx: -1, scroll: 0 });
  app.activeTab = id;
  // Explorer opens new tabs on the Home page.
  navigate(path ?? HOME);
}

export function switchTab(id: number) {
  if (id === app.activeTab) return;
  const t = tabs.find((t) => t.id === id);
  if (!t) return;
  saveScroll();
  app.activeTab = id;
  syncNav();
  // ponytail: re-list on switch (fast) instead of caching per-tab listings
  navigate(t.path, false).then(() => restoreScroll(t.scroll));
}

export function closeTab(id: number) {
  const idx = tabs.findIndex((t) => t.id === id);
  if (idx < 0) return;
  if (tabs[idx].path) closedTabs.push(tabs[idx].path);
  tabs.splice(idx, 1);
  if (!tabs.length) {
    import("@tauri-apps/api/window").then((m) => m.getCurrentWindow().close());
    return;
  }
  if (app.activeTab === id) {
    const next = tabs[Math.min(idx, tabs.length - 1)];
    app.activeTab = next.id;
    syncNav();
    navigate(next.path, false).then(() => restoreScroll(next.scroll));
  }
}
export function duplicateTab(id: number) {
  const t = tabs.find((t) => t.id === id);
  if (t) newTab(t.path);
}

export function closeOtherTabs(id: number) {
  for (const t of [...tabs]) if (t.id !== id) closeTab(t.id);
}

export function closeTabsRight(id: number) {
  const idx = tabs.findIndex((t) => t.id === id);
  if (idx < 0) return;
  for (const t of [...tabs.slice(idx + 1)]) closeTab(t.id);
}

export function up() {
  if (app.path.startsWith("::")) return;
  const t = app.path.replace(/\\$/, "");
  const i = t.lastIndexOf("\\");
  // Explorer: Up from a drive root lands on This PC.
  if (i < 0) {
    navigate(THIS_PC);
    return;
  }
  navigate(i <= 2 ? t.slice(0, 2) + "\\" : t.slice(0, i));
}

export async function sortBy(key: string, asc?: boolean) {
  if (asc === undefined) {
    asc = app.sortKey === key ? !app.asc : true;
  }
  app.sortKey = key;
  app.asc = asc;
  // Explorer behavior: sorting by a date field groups by its date buckets;
  // moving to a non-date sort drops that automatic grouping again.
  if (isDateKey(key) && (app.groupKey === "none" || groupAuto)) {
    app.groupKey = key;
    groupAuto = true;
  } else if (groupAuto && !isDateKey(key)) {
    app.groupKey = "none";
    groupAuto = false;
  }
  try {
    if (app.groupKey !== "none") {
      // Explorer keeps grouping and re-sorts within each group.
      app.groups = await invoke<Group[]>("set_group", {
        key: app.groupKey,
        offsetMin: tzOffsetMin(),
        sortKey: key,
        asc,
      });
    } else {
      app.groups = [];
      await invoke("set_sort", { sortKey: key, asc });
    }
  } catch (e) {
    app.status = String(e); // e.g. property sort over the size cap
  }
  savePrefs();
  app.version++;
  rows.length = 0;
  rows.length = app.total;
  clearSel();
}

// Minutes to add to a UTC timestamp to reach local time (DST-correct for today),
// so the backend's date buckets fall on the user's calendar-day boundaries.
function tzOffsetMin(): number {
  return -new Date().getTimezoneOffset();
}

export async function setGroup(key: string) {
  app.groupKey = key;
  groupAuto = false;
  collapsedGroups.clear();
  app.groups =
    key === "none"
      ? []
      : await invoke<Group[]>("set_group", {
          key,
          offsetMin: tzOffsetMin(),
          sortKey: app.sortKey,
          asc: app.asc,
        });
  if (key === "none") {
    await invoke("set_sort", { sortKey: app.sortKey, asc: app.asc }).catch((e) => (app.status = String(e)));
  }
  savePrefs();
  app.version++;
  rows.length = 0;
  rows.length = app.total;
  clearSel();
  hooks.scrollTo?.(0);
}

async function reapplyGroup() {
  if (app.groupKey === "none") return;
  app.groups = await invoke<Group[]>("set_group", {
    key: app.groupKey,
    offsetMin: tzOffsetMin(),
    sortKey: app.sortKey,
    asc: app.asc,
  });
  app.version++;
  rows.length = 0;
  rows.length = app.total;
}

let filterTimer: ReturnType<typeof setTimeout> | undefined;
export function setFilter(q: string) {
  if (app.path.startsWith("::")) return; // virtual pages have no listing to search
  app.filter = q;
  clearTimeout(filterTimer);
  filterTimer = setTimeout(async () => {
    if (!q.trim()) {
      app.searching = false;
      refresh(false);
      return;
    }
    app.searching = true;
    await invoke("search_start", { query: q.trim() });
    resetRows(0);
    app.filter = q;
    hooks.scrollTo?.(0);
  }, 300);
}

/// Called from the "search-update" event: results are append-only, so cached
/// rows keep their indices and we only extend the row array.
export function searchUpdate(total: number, done: boolean) {
  if (!app.searching) return;
  app.total = total;
  rows.length = total;
  if (done) app.searching = false;
}

export function ensureRange(start: number, end: number) {
  start = Math.max(0, start);
  end = Math.min(app.total, end);
  if (start >= end) return;
  const c0 = Math.floor(start / CHUNK);
  const c1 = Math.floor((end - 1) / CHUNK);
  for (let c = c0; c <= c1; c++) {
    const key = `${app.version}:${c}`;
    if (pending.has(key)) continue;
    const cStart = c * CHUNK;
    const cEnd = Math.min(app.total, cStart + CHUNK);
    if (rows[cStart] !== undefined && rows[cEnd - 1] !== undefined) continue;
    pending.add(key);
    const v = app.version;
    invoke<Entry[]>("get_range", { start: cStart, count: CHUNK })
      .then((got) => {
        if (app.version !== v) return;
        for (let i = 0; i < got.length; i++) rows[cStart + i] = got[i];
      })
      .finally(() => pending.delete(key));
  }
}

// exe/lnk/ico can have unique per-file icons; everything else shares by extension.
const PATH_ICON_EXTS = new Set(["exe", "lnk", "ico", "url"]);

export function requestMeta(e: Entry) {
  const key = e.is_dir ? "//dir" : e.ext;
  if (!extMeta.has(key)) {
    extMeta.set(key, { icon: null, type_name: "" });
    invoke<ExtMeta>("ext_meta", { ext: e.is_dir ? "" : e.ext, isDir: e.is_dir }).then((m) =>
      extMeta.set(key, m),
    );
  }
  if (!e.is_dir && PATH_ICON_EXTS.has(e.ext)) {
    const p = entryPath(e);
    if (!pathIcons.has(p)) {
      if (pathIcons.size > 2000) pathIcons.clear(); // ponytail: dumb cap; LRU if it ever matters
      pathIcons.set(p, null);
      invoke<string | null>("path_icon", { path: p }).then((ic) => pathIcons.set(p, ic));
    }
  }
}

export function iconOf(e: Entry): string | null {
  if (!e.is_dir && PATH_ICON_EXTS.has(e.ext)) {
    const ic = pathIcons.get(entryPath(e));
    if (ic) return ic;
  }
  return extMeta.get(e.is_dir ? "//dir" : e.ext)?.icon ?? null;
}

export function requestThumb(i: number, e: Entry, size: number) {
  const key = entryPath(e) + "|" + size;
  if (thumbs.has(key)) return;
  if (thumbs.size > 700) thumbs.clear(); // ponytail: dumb cap; LRU if memory matters
  thumbs.set(key, null);
  invoke<string | null>("thumb", { index: i, size }).then((t) => {
    if (t) thumbs.set(key, t);
  });
}

export function thumbOf(e: Entry, size: number): string | null {
  return thumbs.get(entryPath(e) + "|" + size) ?? iconOf(e);
}

export function typeOf(e: Entry): string {
  const m = extMeta.get(e.is_dir ? "//dir" : e.ext);
  if (m?.type_name) return m.type_name;
  if (e.is_dir) return "File folder";
  return e.ext ? e.ext.toUpperCase() + " File" : "File";
}

export function fmtDate(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  return (
    d.toLocaleDateString(undefined, { day: "2-digit", month: "2-digit", year: "numeric" }) +
    " " +
    d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })
  );
}

export function fmtSizeKB(e: Entry): string {
  if (e.is_dir) return "";
  return Math.max(e.size > 0 ? 1 : 0, Math.ceil(e.size / 1024)).toLocaleString() + " KB";
}

export function fmtSizeAuto(bytes: number): string {
  if (bytes < 1024) return bytes + " bytes";
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${units[u]}`;
}

// ---- operations ----

export function openEntry(i: number) {
  const e = rows[i];
  if (!e) return;
  const p = entryPath(e);
  if (e.is_dir) navigate(p);
  else invoke("open_path", { path: p }).catch((err) => (app.status = String(err)));
}

export async function refreshClipState() {
  app.canPaste = await invoke<boolean>("clipboard_has_files").catch(() => false);
}

export async function copySel(cut = false) {
  if (!selCount()) return;
  try {
    await invoke("clip_copy", { indices: selIndices(), cut });
  } catch (e) {
    app.status = String(e);
  }
  refreshClipState();
}

// ---- undo (Ctrl+Z) ----
// Each performed operation batch records what actually happened (real
// destination names from the IFileOperation sink); undo inverts the batch.
export interface OpRecord {
  kind: string; // "copy" | "move" | "rename" | "delete" | "new"
  src: string;
  dst: string;
}
const undoStack: OpRecord[][] = [];
export function pushUndo(records: OpRecord[]) {
  if (!records.length) return;
  undoStack.push(records);
  if (undoStack.length > 20) undoStack.shift();
}

export async function undo() {
  const recs = undoStack.pop();
  if (!recs) {
    app.status = "Nothing to undo";
    return;
  }
  try {
    // Deleted items come back from the recycle bin (matched by name).
    const deleted = recs
      .filter((r) => r.kind === "delete")
      .map((r) => r.src.split("\\").pop()!)
      .filter(Boolean);
    if (deleted.length) await invoke("recycle_restore", { names: deleted });
    // Created copies / New items go to the recycle bin (still recoverable).
    const created = recs.filter((r) => r.kind === "copy" || r.kind === "new").map((r) => r.dst);
    if (created.length) await invoke("delete_paths", { paths: created });
    // Moves and renames go straight back.
    for (const r of recs.filter((r) => r.kind === "move" || r.kind === "rename")) {
      await invoke("move_path", { from: r.dst, to: r.src });
    }
    app.status = "Undone";
  } catch (e) {
    app.status = "Undo: " + String(e);
  }
  refresh();
}

export async function paste() {
  if (!app.canPaste) return;
  try {
    pushUndo(await invoke<OpRecord[]>("clip_paste", { dest: app.path }));
  } catch (e) {
    app.status = String(e);
  }
  refresh();
}

export async function del() {
  if (!selCount()) return;
  try {
    pushUndo(await invoke<OpRecord[]>("delete_items", { indices: selIndices() }));
  } catch (e) {
    app.status = String(e);
  }
  refresh();
}

export async function newFolder() {
  try {
    const name = await invoke<string>("create_folder", { dir: app.path });
    pushUndo([{ kind: "new", src: "", dst: joinPath(app.path, name) }]);
    await refresh();
    const idx = await invoke<number | null>("find_index", { name });
    if (idx != null) {
      clearSel();
      sel.add(idx);
      app.anchor = idx;
      app.renaming = idx;
      hooks.scrollToIndex?.(idx);
    }
  } catch (e) {
    app.status = String(e);
  }
}

export interface NewItem {
  ext: string;
  name: string;
}
export const newItems = $state<{ list: NewItem[] | null }>({ list: null });

export async function loadNewItems() {
  if (newItems.list) return;
  newItems.list = await invoke<NewItem[]>("new_menu_items").catch(() => []);
  for (const it of newItems.list ?? []) requestExtMeta(it.ext);
}

export function requestExtMeta(ext: string) {
  if (extMeta.has(ext)) return;
  extMeta.set(ext, { icon: null, type_name: "" });
  invoke<ExtMeta>("ext_meta", { ext, isDir: false }).then((m) => extMeta.set(ext, m));
}

export async function newFile(ext: string) {
  try {
    const name = await invoke<string | null>("create_new", { dir: app.path, ext });
    if (!name) return; // command-driven type: a wizard was launched instead
    pushUndo([{ kind: "new", src: "", dst: joinPath(app.path, name) }]);
    await refresh();
    const idx = await invoke<number | null>("find_index", { name });
    if (idx != null) {
      clearSel();
      sel.add(idx);
      app.anchor = idx;
      app.renaming = idx;
      hooks.scrollToIndex?.(idx);
    }
  } catch (e) {
    app.status = String(e);
  }
}

export async function commitRename(index: number, newName: string) {
  app.renaming = -1;
  const e = rows[index];
  if (!e || !newName.trim() || newName === e.name) return;
  try {
    const oldPath = entryPath(e);
    await invoke("rename_item", { index, newName });
    pushUndo([{ kind: "rename", src: oldPath, dst: joinPath(e.parent ?? app.path, newName.trim()) }]);
    await refresh();
    const idx = await invoke<number | null>("find_index", { name: newName.trim() });
    if (idx != null) {
      clearSel();
      sel.add(idx);
      app.anchor = idx;
    }
  } catch (err) {
    app.status = String(err);
  }
}

export function startRename() {
  if (selCount() === 1) app.renaming = selIndices()[0];
}

export function selectAll() {
  sel.clear();
  app.allSelected = true;
}

// E2E hook: lets automated tests drive the app over CDP (harmless local debug affordance).
{
  (globalThis as any).__e2e = {
    app, sel, rows, tabs, navigate, refresh, back, forward, up, sortBy, setFilter,
    selectAll, clearSel, selCount, openEntry, newFolder, del, copySel, paste,
    startRename, commitRename, refreshClipState, newTab, switchTab, closeTab,
    setGroup, collapsedGroups, loadNewItems, newFile, newItems, invoke, setTheme, setView,
    undo, pushUndo,
  };
}
