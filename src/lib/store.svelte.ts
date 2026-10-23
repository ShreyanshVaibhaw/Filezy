import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { settings, setSetting, setTheme } from "./settings.svelte";
import { baseName, isArchive, isServer, isVirtual, joinPath, parentPath, samePath, pathKey } from "./paths";
import { formatWith, type LocaleFormats } from "./datefmt";
import { clearColumns } from "./columns";
import { withFilter, withoutFilters } from "./searchquery";
import { DEVICES, isShellLocation, shellLocation, shellParsing, type ShellFolder } from "./shellplaces";
import { transferTab } from "./tabtransfer";

export { joinPath } from "./paths";

export interface Entry {
  name: string;
  is_dir: boolean;
  size: number;
  modified: number;
  created: number;
  ext: string;
  attrs: number; // FILE_ATTRIBUTE_* bits
  parent?: string; // set on search results
}
export const ATTR_READONLY = 0x1;
export const ATTR_HIDDEN = 0x2;
export const ATTR_REPARSE = 0x400;

export interface ExtMeta {
  icon: string | null;
  type_name: string;
}
export interface Drive {
  path: string;
  name: string;
  kind: "fixed" | "removable" | "optical" | "network" | "ram";
  total: number;
  free: number;
  /** ready; empty (no disc or card); offline (network drive not connected); slow (didn't answer); error. */
  state: "ready" | "empty" | "offline" | "slow" | "error";
  /** Windows' words for why it can't be read. */
  note: string | null;
  read_only: boolean;
  ejectable: boolean;
  /** The share a network drive maps. */
  remote: string | null;
}

/** A computer the Network page found: "\\NAME". */
export interface Place {
  name: string;
  path: string;
}

export interface Tab {
  id: number;
  path: string;
  hist: string[];
  histIdx: number;
  scroll: number;
  /** Sequence of this tab's in-flight navigation, 0 when settled. */
  pending: number;
  /** Search box text, so the tab's results come back when it is reselected. */
  query: string;
  /** Selection to restore when the tab is listed again. */
  keepKeys: string[];
  keepAll: boolean;
}
export interface Group {
  label: string;
  start: number;
  count: number;
}

/** A selection (or other rows) as of one listing revision; see listing.rs Keep. */
export interface Keep {
  rev: number;
  /** The selection. */
  idx?: number[];
  all?: boolean;
  /** Single rows by index, here [anchor, renaming]; -1 for none. */
  focus?: number[];
  /** Rows by identity key. */
  names?: string[];
}
interface Kept {
  sel: number[];
  focus: number[];
  names: number[];
}

export interface SearchInfo {
  seq: number;
  root: string;
  query: string;
  done: boolean;
  stopped: boolean;
  capped: boolean;
  skipped: number;
  provider: string;
  /** What the search couldn't cover (no index, so no file contents...). */
  note: string;
}

/** Where a search looks: this folder, it and its subfolders, its drive, or every drive. */
export type SearchScope = "folder" | "subfolders" | "drive" | "all";

export const app = $state({
  path: "",
  home: "",
  /** Revision of the backend listing the cached rows belong to. */
  rev: 0,
  total: 0,
  /** Bumped whenever cached rows must be dropped and re-fetched. */
  version: 0,
  sortKey: "name",
  asc: true,
  view: "details",
  /** Search box text. */
  filter: "",
  /** The search whose results are listed, if any. */
  search: null as SearchInfo | null,
  searching: false,
  groupKey: "none",
  groups: [] as Group[],
  histIdx: -1,
  histLen: 0,
  activeTab: 0,
  canPaste: false,
  /** The scope of the next search; drive and all-drive scopes last one search session. */
  searchScope: settings.searchScope as SearchScope,
  /** Lowercased paths of files cut to the clipboard, by any app. */
  cut: new Set<string>(),
  renaming: -1,
  /** Where a Shift range starts. */
  anchor: 0,
  /** The row the keyboard is on (Explorer's focus rectangle); -1 for none. */
  focus: -1,
  // ponytail: select-all is a flag, not 100k Set inserts; per-item ops resolve through selRef()
  allSelected: false,
  /** A navigation of the active tab is in flight. */
  loading: false,
  /** Why the current location couldn't be listed; shown in place of the list. */
  error: "",
  /** False when Windows can't report changes here, so the view won't refresh itself. */
  live: true,
  status: "",
  /** Equals `status` when the message is information rather than an error (see say()). */
  statusInfo: "",
  drives: [] as Drive[],
  specials: [] as [string, string][],
  pins: [] as string[],
  /** Files added to Favorites, shown on Home. */
  favorites: [] as string[],
  /** Cloud folders providers registered (OneDrive, Dropbox...), named as Explorer names them. */
  cloudRoots: [] as { path: string; name: string }[],
  /** The current folder is in one a cloud provider syncs: its items have a Status. */
  cloud: false,
});

/** Show an informational message (not an error) in the message bar. */
export function say(text: string) {
  app.statusInfo = text;
  app.status = text;
}

export const tabs = $state<Tab[]>([]);
let nextTabId = 1;
export function activeTab(): Tab | undefined {
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
/** The selection as the backend resolves it: indices of one listing revision. */
export function selRef(): Keep {
  return app.allSelected ? { rev: app.rev, all: true } : { rev: app.rev, idx: [...sel] };
}
export function firstSel(): number | undefined {
  if (app.allSelected) return app.total ? 0 : undefined;
  let min = -1;
  for (const k of sel) if (min < 0 || k < min) min = k;
  return min < 0 ? undefined : min;
}
function selectOnly(i: number) {
  clearSel();
  sel.add(i);
  app.anchor = i;
}

// Sparse cache of listing rows; holes are unfetched. Never holds more than what was scrolled past.
export const rows = $state<(Entry | undefined)[]>([]);
export const extMeta = new SvelteMap<string, ExtMeta>();
export const pathIcons = new SvelteMap<string, string | null>();

// Thumbnails for icon/tile views, keyed "path|size|modified" so an edited file gets a new one.
export const thumbs = new SvelteMap<string, string | null>();

// ---- property columns ----
// Values of property columns (Tags, Title, Length...) for the rows on screen,
// keyed by path, modified time and property: an edited file gets new keys.
export const propVals = new SvelteMap<string, string | null>();
const propQueue = new Map<string, [string, number]>();
let propKeys: string[] = [];
let propTimer = 0;

/// Ask for these properties of a row; requests made within a frame go together.
export function requestProps(e: Entry, keys: string[]) {
  if (!keys.length || e.is_dir) return;
  const p = entryPath(e);
  if (keys.every((k) => propVals.has(`${p}|${e.modified}|${k}`))) return;
  if (propVals.size > 50_000) propVals.clear(); // ponytail: dumb cap; LRU if it ever matters
  for (const k of keys) propVals.set(`${p}|${e.modified}|${k}`, null);
  propQueue.set(`${p}|${e.modified}`, [p, e.modified]);
  propKeys = keys;
  propTimer ||= window.setTimeout(flushProps, 16);
}

async function flushProps() {
  propTimer = 0;
  const items = [...propQueue.values()];
  const keys = propKeys;
  propQueue.clear();
  const vals = await invoke<(string | null)[][]>("props_for", { items, keys }).catch(() => []);
  items.forEach(([p, m], i) => keys.forEach((k, j) => propVals.set(`${p}|${m}|${k}`, vals[i]?.[j] ?? null)));
}

export function propOf(e: Entry, key: string): string {
  return propVals.get(`${entryPath(e)}|${e.modified}|${key}`) ?? "";
}

// ---- cloud status ----
// Explorer's Status for rows on screen in cloud folders ("Available when
// online", "Sync pending"...), read from the shell. Syncing changes it without
// touching the file, so it's asked again for every listing (each change Windows
// reports re-lists the folder); the last answer shows meanwhile.
/** STORAGE_PROVIDER_STATE (props.rs CloudState) and its wording. */
export interface CloudState {
  state: number;
  text: string;
}
const cloudStates = new SvelteMap<string, { rev: number; got: CloudState | null }>();
const statusQueue = new Set<string>();
let statusTimer = 0;

/** Whether a row can have a cloud state: every item of a synced folder can,
 *  and elsewhere (search results, libraries) the ones whose attributes say so
 *  (cloud.rs may_have_state). Windows hides the placeholder bit of a downloaded
 *  file from apps, so in a synced folder the attributes alone aren't enough. */
const mayHaveState = (e: Entry) => app.cloud || (e.attrs & (0x400 | 0x1000 | 0x40000 | 0x80000 | 0x100000 | 0x400000)) !== 0;

/// Ask for a row's Status; requests made within a frame go together.
export function requestStatus(e: Entry) {
  if (!mayHaveState(e)) return;
  const p = entryPath(e);
  const known = cloudStates.get(p);
  if (known?.rev === app.rev) return;
  if (cloudStates.size > 20_000) cloudStates.clear(); // ponytail: dumb cap, as for properties
  cloudStates.set(p, { rev: app.rev, got: known?.got ?? null });
  statusQueue.add(p);
  statusTimer ||= window.setTimeout(flushStatus, 16);
}

async function flushStatus() {
  statusTimer = 0;
  const rev = app.rev;
  const paths = [...statusQueue];
  statusQueue.clear();
  const vals = await invoke<(CloudState | null)[]>("cloud_status", { paths }).catch(() => []);
  paths.forEach((p, i) => cloudStates.set(p, { rev, got: vals[i] ?? null }));
}

export function cloudStateOf(e: Entry): CloudState | null {
  return mayHaveState(e) ? (cloudStates.get(entryPath(e))?.got ?? null) : null;
}

export function statusOf(e: Entry): string {
  return cloudStateOf(e)?.text ?? "";
}

/** Whether a row is set to Always keep on this device. */
export const isKept = (e: Entry) => (e.attrs & 0x80000) !== 0;

// The user's Windows date and time formats, once the backend has said them.
export const locale = $state<{ f: LocaleFormats | null }>({ f: null });
invoke<LocaleFormats>("locale_formats")
  .then((f) => (locale.f = f))
  .catch(() => {});

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
      invoke<string | null>("path_thumb", { path, size })
        .then((ic) => {
          if (ic) shellIcons.set(key, ic);
        })
        .catch(() => {});
    });
  }
  return v ?? null;
}

// Callbacks registered by FilePane so store actions can control the scroller.
export const hooks: {
  scrollTo?: (px: number) => void;
  scrollToIndex?: (i: number) => void;
  getScroll?: () => number;
  /** Entry indices the pane shows now, and how many fit from the top. */
  visibleRange?: () => { first: number; last: number };
  visibleCount?: () => number;
  gridCols?: number;
  /** Rows to move for Left/Right: 0 where items don't sit side by side. */
  hStep?: number;
  /** Rows to move for Page Up/Down. */
  pageSize?: number;
  /** Visual rows a page holds, for moveVertical. */
  pageRows?: number;
  /** Where Up, Down or a page (`rows` visual rows, negative up) from item i lands, group-aware. */
  moveVertical?: (i: number, rows: number) => number;
  /** Open the context menu for the focused row from the keyboard. */
  contextMenu?: () => void;
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
export const SORT_KEYS = [
  "name", "modified", "type", "size", "created", "author", "category", "tags", "title",
  "location", "attributes", "status", "dimensions", "duration",
];
export const VIEW_KEYS = ["details", "content", "list", "tiles", "icons_s", "icons_m", "icons_l", "icons_xl", "gallery"];
export const GROUP_KEYS = ["none", "name", "modified", "created", "type", "size", "month"];

function validPrefs(v: unknown): v is FolderPrefs {
  const p = v as FolderPrefs;
  return (
    !!p &&
    typeof p === "object" &&
    SORT_KEYS.includes(p.s) &&
    typeof p.a === "boolean" &&
    VIEW_KEYS.includes(p.v) &&
    GROUP_KEYS.includes(p.g)
  );
}

/// Saved views, with anything malformed dropped (and the cleaned set written
/// back): a corrupt blob must not stop startup or linger.
function loadFolderPrefs(): Record<string, FolderPrefs> {
  const out: Record<string, FolderPrefs> = {};
  try {
    const raw = JSON.parse(localStorage.getItem("folderPrefs") ?? "{}");
    const entries = raw && typeof raw === "object" && !Array.isArray(raw) ? Object.entries(raw) : [];
    for (const [k, v] of entries) if (validPrefs(v)) out[k] = v;
    if (entries.length !== Object.keys(out).length) localStorage.setItem("folderPrefs", JSON.stringify(out));
  } catch {
    localStorage.removeItem("folderPrefs");
  }
  return out;
}

/// Fallback view for a folder the user has never customized, from Settings.
function defaultPrefs(): FolderPrefs {
  if (settings.defaultView === "gallery") return { s: "modified", a: false, v: "gallery", g: "month" };
  return { s: settings.defaultSort, a: settings.defaultAsc, v: settings.defaultView, g: "none" };
}
let folderPrefs: Record<string, FolderPrefs> = loadFolderPrefs();

/// Explorer's "Reset Folders": every folder falls back to the configured default.
export function clearFolderPrefs() {
  folderPrefs = {};
  localStorage.removeItem("folderPrefs");
  clearColumns();
}
export function folderPrefsCount(): number {
  return Object.keys(folderPrefs).length;
}
// True while the current grouping came from date-sorting, not a user choice.
let groupAuto = false;

function prefKey(p: string): string {
  return pathKey(p);
}

function savePrefs() {
  if (!app.path || isVirtual(app.path)) return;
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

/// The arrangement a folder opens with: its saved prefs or the configured default.
function prefsFor(p: string): { sort: string; asc: boolean; view: string; group: string; auto: boolean } {
  const prefs = folderPrefs[prefKey(p)] ?? defaultPrefs();
  // Explorer behavior: date-sorted folders group by date automatically.
  const auto = prefs.g === "none" && isDateKey(prefs.s);
  return { sort: prefs.s, asc: prefs.a, view: prefs.v, group: auto ? prefs.s : prefs.g, auto };
}

// The arrangement a folder had before Gallery replaced it, to go back to.
let beforeGallery: { path: string; sort: string; asc: boolean; group: string; auto: boolean } | null = null;

/// Gallery brings its own arrangement, newest first in month groups; leaving
/// it brings back the folder's arrangement from before, or just drops the
/// month groups when the folder opened in Gallery.
export async function setView(v: string) {
  const was = app.view;
  app.view = v;
  let auto = false;
  if (v === "gallery" && app.groupKey !== "month") {
    beforeGallery = { path: app.path, sort: app.sortKey, asc: app.asc, group: app.groupKey, auto: groupAuto };
    Object.assign(app, { sortKey: "modified", asc: false, groupKey: "month" });
  } else if (was === "gallery" && v !== "gallery" && beforeGallery && samePath(beforeGallery.path, app.path)) {
    Object.assign(app, { sortKey: beforeGallery.sort, asc: beforeGallery.asc, groupKey: beforeGallery.group });
    auto = beforeGallery.auto;
    beforeGallery = null;
  } else if (was === "gallery" && v !== "gallery" && app.groupKey === "month") {
    app.groupKey = "none";
  } else {
    return savePrefs();
  }
  groupAuto = auto;
  collapsedGroups.clear();
  if (!app.search) savePrefs();
  await arrangeNow(true);
}

// Reopen-closed-tab stack (Ctrl+Shift+T), session-only.
const closedTabs: string[] = [];
export function reopenClosedTab() {
  const p = closedTabs.pop();
  if (p) newTab(p);
}

// Virtual locations (no backend listing).
export const THIS_PC = "::ThisPC";
export const NETWORK = "::Network";
export const HOME = "::Home";
export const RECYCLE = "::RecycleBin";
export const SETTINGS = "::Settings";
// Shell GUID of the bin itself, for its real icon.
export const RECYCLE_GUID = "::{645FF040-5081-101B-9F08-00AA002F954E}";

/** Commands a location supports, so shortcuts and buttons can't aim New, Paste
 * or Delete at a page that has no folder behind it. */
/// What the current location allows. "change" is cutting, deleting, renaming
/// and the like on the items shown.
export function can(cmd: "new" | "paste" | "select" | "search" | "refresh" | "up" | "change"): boolean {
  const p = app.path;
  if (!p || app.error) return cmd === "refresh" || cmd === "up";
  if (isShellLocation(p)) return cmd === "refresh" || cmd === "up";
  if (isVirtual(p)) return cmd === "refresh";
  // Inside an archive: look, open and extract; nothing in it changes yet.
  // A computer's shares are just as fixed: Windows makes them, not Filezy.
  if (isArchive(p) || isServer(p)) return cmd === "select" || cmd === "refresh" || cmd === "up";
  if (app.search) return cmd === "select" || cmd === "search" || cmd === "refresh" || cmd === "up" || cmd === "change";
  return true;
}

export interface RecycleItem {
  /** Opaque identity in the bin; never the display name. */
  id: string;
  name: string;
  original: string;
  deleted: string;
  size: string;
}
export const recycleItems = $state<{ list: RecycleItem[]; total: number; loading: boolean }>({
  list: [],
  total: 0,
  loading: false,
});

export interface RecentFile {
  name: string;
  path: string;
  ext: string;
  modified: number;
}
export const recentFiles = $state<{ list: RecentFile[] }>({ list: [] });

export function folderName(p: string): string {
  if (p === DEVICES) return "Devices";
  if (isShellLocation(p)) return p === app.path && shellFolder.name ? shellFolder.name : "Device folder";
  if (p === THIS_PC) return "This PC";
  if (p === NETWORK) return "Network";
  if (p === HOME) return "Home";
  if (p === RECYCLE) return "Recycle Bin";
  if (p === SETTINGS) return "Settings";
  return baseName(p);
}

export function entryPath(e: Entry): string {
  return joinPath(e.parent ?? app.search?.root ?? app.path, e.name);
}

export function isHidden(e: Entry): boolean {
  return (e.attrs & ATTR_HIDDEN) !== 0;
}

/// Name as shown in the panes: extension stripped when "File name extensions" is off.
export function displayName(e: Entry): string {
  if (settings.showExt || e.is_dir || !e.ext) return e.name;
  const cut = e.name.length - e.ext.length - 1;
  return cut > 0 ? e.name.slice(0, cut) : e.name;
}

export function toggleHidden() {
  setSetting("showHidden", !settings.showHidden);
  refresh();
}

// ---- Quick Access pins ----

export async function loadPins() {
  app.pins = await invoke<string[]>("get_pins").catch(() => []);
  app.favorites = await invoke<string[]>("get_pins", { kind: "favorites" }).catch(() => []);
}
/// Move a pin to another place in the list, as dragging it in Quick access does.
export async function movePin(from: number, to: number) {
  if (from === to || from < 0 || to < 0 || from >= app.pins.length || to >= app.pins.length) return;
  const list = [...app.pins];
  list.splice(to, 0, ...list.splice(from, 1));
  app.pins = list;
  await invoke("set_pins", { pins: app.pins }).catch((e) => (app.status = String(e)));
}

// ---- Favorites: files kept on Home, as Explorer's Add to Favorites ----
export function isFavorite(path: string): boolean {
  return app.favorites.some((p) => samePath(p, path));
}
export async function setFavorite(paths: string[], on: boolean) {
  app.favorites = on
    ? [...app.favorites, ...paths.filter((p) => !isFavorite(p))]
    : app.favorites.filter((f) => !paths.some((p) => samePath(p, f)));
  await invoke("set_pins", { pins: app.favorites, kind: "favorites" }).catch((e) => (app.status = String(e)));
}

// ---- Recent files ----
/// Take files off Recent files (Windows' Recent items, which Explorer's Home
/// and Start show too); the files themselves stay.
export async function removeRecent(paths: string[]) {
  await invoke("recent_remove", { paths }).catch((e) => (app.status = String(e)));
  recentFiles.list = await invoke<RecentFile[]>("recent_files").catch(() => recentFiles.list);
}
/// Clear Recent files entirely; returns how many entries went.
export async function clearRecent(): Promise<number> {
  const n = await invoke<number>("recent_clear").catch((e) => ((app.status = String(e)), 0));
  recentFiles.list = [];
  return n;
}
export function isPinned(path: string): boolean {
  return app.pins.some((p) => samePath(p, path));
}
export async function pinAdd(path: string) {
  if (isPinned(path)) return;
  app.pins = [...app.pins, path];
  await invoke("set_pins", { pins: app.pins }).catch((e) => (app.status = String(e)));
}
export async function pinRemove(path: string) {
  app.pins = app.pins.filter((p) => !samePath(p, path));
  await invoke("set_pins", { pins: app.pins }).catch((e) => (app.status = String(e)));
}

/** Drop the cached rows: the listing's indices changed meaning. */
function resetRows(total: number) {
  app.total = total;
  app.version++;
  rows.length = 0;
  rows.length = total;
}

interface Prefetched {
  start: number;
  rows: Entry[];
}

/// Fetch the rows the pane is about to show from a new listing revision, so
/// swapping the row cache never leaves a frame with empty rows (and an open
/// rename box never disappears under the user).
async function prefetch(rev: number, total: number, keepScroll: boolean): Promise<Prefetched> {
  const r = keepScroll ? hooks.visibleRange?.() : { first: 0, last: hooks.visibleCount?.() ?? 64 };
  const start = Math.min(r?.first ?? 0, total);
  const count = Math.min(total, r?.last ?? 64) - start;
  if (count <= 0) return { start, rows: [] };
  const got = await invoke<Entry[]>("get_range", { rev, start, count }).catch(() => [] as Entry[]);
  return { start, rows: got };
}

function installRows(total: number, pre: Prefetched) {
  resetRows(total);
  pre.rows.forEach((e, k) => (rows[pre.start + k] = e));
}

/// The drives for This PC and the navigation pane, read again whenever
/// Windows says a drive came or went.
export async function loadDrives() {
  app.drives = await invoke<Drive[]>("drives").catch(() => app.drives);
}

/** What the Network page shows: the computers Windows found, once it has looked. */
export const network = $state<{ places: Place[]; loading: boolean; error: string }>({ places: [], loading: false, error: "" });

export async function loadNetwork() {
  network.loading = true;
  network.error = "";
  try {
    network.places = await invoke<Place[]>("network_places");
  } catch (e) {
    network.places = [];
    network.error = String(e);
  } finally {
    network.loading = false;
  }
}

async function loadVirtual(p: string) {
  if (isShellLocation(p)) await loadShell(p);
  if (p === THIS_PC) loadDrives();
  if (p === NETWORK) loadNetwork();
  if (p === HOME) {
    invoke<RecentFile[]>("recent_files")
      .then((r) => (recentFiles.list = r))
      .catch((e) => (app.status = `Recent files couldn't be read. ${e}`));
  }
  if (p === RECYCLE) loadRecycle();
}

export const shellFolder = $state<ShellFolder & { loading: boolean; error: string }>({ name: "", parent: null, items: [], capped: false, loading: false, error: "" });
export const shellSelection = $state<{ paths: string[] }>({ paths: [] });
let shellSeq = 0;
async function loadShell(path: string) {
  const seq = ++shellSeq;
  Object.assign(shellFolder, { name: "", parent: null, items: [], loading: true, error: "", capped: false });
  try {
    const result = await invoke<ShellFolder>("shell_list", { path: shellParsing(path), devicesOnly: path === DEVICES });
    if (seq === shellSeq && app.path === path) {
      Object.assign(shellFolder, result);
      const paths = new Set(result.items.map((item) => item.path));
      shellSelection.paths = shellSelection.paths.filter((path) => paths.has(path));
    }
  } catch (error) { if (seq === shellSeq) shellFolder.error = String(error); }
  finally { if (seq === shellSeq) shellFolder.loading = false; }
}

export async function loadRecycle() {
  recycleItems.loading = true;
  try {
    const r = await invoke<{ items: RecycleItem[]; total: number }>("recycle_list");
    recycleItems.list = r.items;
    recycleItems.total = r.total;
  } catch (e) {
    app.status = String(e);
  } finally {
    recycleItems.loading = false;
  }
}

// ---- navigation ----
// Every request that replaces the backend listing (navigation, refresh, tab
// switch, search) takes the next sequence number. The backend never lets an
// older one replace a newer one's listing, and a tab only commits the result of
// its own newest request.
// The numbering continues from a base the backend hands each page load, so
// after a reload this page's requests outrank the old page's.

let seqCounter = -1; // until the base arrives
const seqBase = invoke<number>("seq_base").then(
  (b) => (seqCounter = b),
  () => (seqCounter = 0),
);
/** Only a page's very first request waits for the base. */
const firstSeq = () => seqBase.then(() => ++seqCounter);

function tzOffsetMin(): number {
  // Minutes to add to a UTC timestamp to reach local time (DST-correct for today),
  // so the backend's date groups fall on the user's calendar days.
  return -new Date().getTimezoneOffset();
}

interface Opened {
  path: string;
  rev: number;
  total: number;
  groups: Group[];
  kept: Kept;
  installed: boolean;
  live: boolean;
  notice: string | null;
  cloud: boolean;
}

function pushHistory(t: Tab, p: string) {
  if (t.hist[t.histIdx] !== undefined && samePath(t.hist[t.histIdx], p)) {
    t.hist[t.histIdx] = p;
    return;
  }
  t.hist.splice(t.histIdx + 1);
  t.hist.push(p);
  // Bounded, so a long session's tab still moves to another window (importTab
  // takes at most 1,000 entries).
  if (t.hist.length > HIST_MAX) t.hist.splice(0, t.hist.length - HIST_MAX);
  t.histIdx = t.hist.length - 1;
}
const HIST_MAX = 200;

export interface NavOpts {
  /** Same folder again: keep scroll, and unless `select` says otherwise, the
   * selection, anchor and an open rename. */
  refresh?: boolean;
  /** Select these items (identity keys, see keyOf) after listing; the first is
   * scrolled into view. E.g. the folder Up came from, or a new folder. */
  select?: string[];
}

/// Open a location in the active tab. Returns false when it could not be opened.
export async function navigate(p: string, push = true, opts: NavOpts = {}): Promise<boolean> {
  const t = activeTab();
  if (!t) return false;
  clearTimeout(filterTimer);
  const seq = seqCounter >= 0 ? ++seqCounter : await firstSeq();
  t.pending = seq;

  if (isVirtual(p)) {
    // Supersede anything still loading or searching for the old location.
    invoke("cancel_listing", { seq }).catch(() => {});
    t.pending = 0;
    if (push) pushHistory(t, p);
    t.path = p;
    t.query = "";
    syncNav();
    app.path = p;
    app.filter = "";
    app.search = null;
    endWideScope();
    app.searching = false;
    app.loading = false;
    app.error = "";
    app.status = "";
    if (isShellLocation(p)) shellSelection.paths = [...(opts.select ?? [])];
    clearSel();
    resetRows(0);
    await loadVirtual(p);
    refreshClipState();
    return true;
  }

  // A refresh keeps the search UI; listing the plain folder under it would make
  // the folder's items look like results, so that counts as a fresh navigation.
  const refreshing = !!opts.refresh && !app.search;
  const prefs = refreshing
    ? { sort: app.sortKey, asc: app.asc, view: app.view, group: app.groupKey, auto: groupAuto }
    : prefsFor(p);
  const keep: Keep = opts.select ? { rev: 0, names: opts.select } : refreshing ? keepNow() : { rev: 0 };
  const sigBefore = refreshing ? selSignature() : "";
  if (!refreshing) app.loading = true;
  let res: Opened;
  try {
    res = await invoke<Opened>("open_dir", {
      req: {
        path: p,
        seq,
        arr: { sort: prefs.sort, asc: prefs.asc, group: prefs.group, tzOffMin: tzOffsetMin() },
        showHidden: settings.showHidden,
        showProtected: settings.showProtected,
        keep,
      },
    });
  } catch (e) {
    if (t.pending !== seq) return false;
    t.pending = 0;
    if (t.id === app.activeTab) {
      app.loading = false;
      const msg = String(e);
      // The folder itself went away (deleted, drive removed): leave it like
      // Explorer, for the nearest ancestor that still opens.
      if (refreshing && msg.startsWith("Windows can't find")) return openNearest(app.path, msg);
      if (refreshing) {
        // The folder is still there but can't be read now: show why, in place,
        // with no stale rows left to act on.
        app.error = msg;
        clearSel();
        app.renaming = -1;
        resetRows(0);
      } else app.status = msg;
    }
    return false;
  }
  if (t.pending !== seq) return true; // a newer navigation in this tab owns it now
  const sameFolder = refreshing || samePath(app.path, res.path);
  const pre =
    res.installed && t.id === app.activeTab ? await prefetch(res.rev, res.total, sameFolder) : null;
  if (t.pending !== seq) return true;
  t.pending = 0;
  const active = t.id === app.activeTab;
  if (!res.installed || !active || !pre) {
    // Superseded by another tab's listing: remember where this tab went, and
    // list it when the tab is shown again.
    if (push) pushHistory(t, res.path);
    t.path = res.path;
    t.keepKeys = opts.select ?? [];
    t.keepAll = false;
    if (active) app.loading = false;
    return true;
  }

  if (push) pushHistory(t, res.path);
  t.path = res.path;
  if (!refreshing) t.query = "";
  syncNav();
  app.path = res.path;
  app.rev = res.rev;
  app.sortKey = prefs.sort;
  app.asc = prefs.asc;
  app.view = prefs.view;
  app.groupKey = prefs.group;
  groupAuto = prefs.auto;
  app.groups = res.groups;
  app.live = res.live;
  app.cloud = res.cloud;
  app.loading = false;
  app.error = "";
  if (!refreshing) {
    app.filter = "";
    app.search = null;
    endWideScope();
    app.searching = false;
    app.status = res.notice ?? "";
    collapsedGroups.clear();
  } else if (res.notice) {
    app.status = res.notice;
  }
  // The user changed the selection while this refresh ran: carry their newer
  // choice over by identity instead of restoring the one captured at the start.
  const moved = refreshing && !opts.select && selSignature() !== sigBefore;
  const nowKeys = moved ? selectionKeys() : null;
  installRows(res.total, pre);
  if (opts.select) selectKept(res.kept);
  else if (nowKeys) await reselect(nowKeys);
  else if (refreshing) restoreKept(res.kept, keep, res.total);
  else {
    clearSel();
    app.anchor = 0;
    app.focus = -1;
    app.renaming = -1;
  }
  if (!sameFolder && !opts.select) hooks.scrollTo?.(0);
  refreshClipState();
  return true;
}

/// After a refresh or re-sort: put the selection, anchor and an open rename
/// back on their rows' new indices. `keep` came from keepNow().
function restoreKept(kept: Kept, keep: Keep, total: number) {
  sel.clear();
  app.allSelected = false;
  if (keep.all && kept.sel.length === total && total > 0) app.allSelected = true;
  else for (const i of kept.sel) sel.add(i);
  app.anchor = Math.max(0, kept.focus[0] ?? 0);
  app.renaming = kept.focus[1] ?? -1;
  app.focus = kept.focus[2] ?? -1;
  if (app.renaming >= 0) hooks.scrollToIndex?.(app.renaming);
}

/// Select the rows that `select` named; the first one found becomes the anchor.
function selectKept(kept: Kept) {
  clearSel();
  app.renaming = -1;
  const found = kept.names.filter((i) => i >= 0);
  for (const i of found) sel.add(i);
  app.anchor = found[0] ?? 0;
  app.focus = found[0] ?? -1;
  if (found.length) hooks.scrollToIndex?.(found[0]);
}

/** Cheap fingerprint of the selection and focus, to notice a change mid-refresh. */
function selSignature(): string {
  if (app.allSelected) return `all|${app.focus}`;
  const ids = [...sel];
  return `${app.focus}|${app.anchor}|${ids.length}|${ids.slice(0, 2000).join(",")}`;
}

/** Identity keys of the selected rows we have (and of the focused row), from the old rows. */
function selectionKeys(): { sel: string[]; focus: string | null; all: boolean } {
  const key = (i: number) => {
    const e = rows[i];
    return e ? (e.parent ? joinPath(e.parent, e.name) : e.name) : null;
  };
  return {
    sel: app.allSelected ? [] : [...sel].map(key).filter((k): k is string => k !== null),
    focus: app.focus >= 0 ? key(app.focus) : null,
    all: app.allSelected,
  };
}

/// Re-apply a selection, by identity, onto the newly installed listing.
async function reselect(k: { sel: string[]; focus: string | null; all: boolean }) {
  clearSel();
  if (k.all) {
    app.allSelected = true;
    return;
  }
  const keys = k.focus ? [...k.sel, k.focus] : k.sel;
  const at = await invoke<number[]>("find_keys", { rev: app.rev, keys }).catch(() => [] as number[]);
  for (const i of at.slice(0, k.sel.length)) if (i >= 0) sel.add(i);
  const f = k.focus ? (at[k.sel.length] ?? -1) : -1;
  app.focus = f;
  if (f >= 0) app.anchor = f;
}

/// Select rows by identity key (listing.rs Entry::key) once the listing and a
/// running search have settled; the first one gets the focus.
export async function selectKeys(keys: string[]) {
  await settle(() => app.searching || activeTab()?.pending || arranging, 10_000);
  if (!keys.length) return;
  await reselect({ sel: keys, focus: keys[0], all: false });
  if (app.focus >= 0) hooks.scrollToIndex?.(app.focus);
}

/// The current selection, anchor and open rename, to carry through a re-list.
/// The backend resolves the indices against this revision, so rows that were
/// never fetched still come along.
function keepNow(): Keep {
  const focus = [app.anchor, app.renaming, app.focus];
  return app.allSelected ? { rev: app.rev, all: true, focus } : { rev: app.rev, idx: [...sel], focus };
}

/// `from` was deleted or disconnected: open the nearest folder above it that
/// still opens, as Explorer does, and say why. With `orHome`, Home when none
/// does, for a tab that has nothing of its own to show; otherwise the reason
/// stays in place of the list.
async function openNearest(from: string, why: string, orHome = false): Promise<boolean> {
  let p = parentPath(from);
  while (p) {
    if (await navigate(p, true)) {
      app.status = why;
      return true;
    }
    p = parentPath(p);
  }
  if (orHome && (await navigate(HOME, true))) {
    app.status = why;
    return true;
  }
  app.error = why;
  return false;
}

/// Re-list the current location, keeping selection, scroll and an open rename.
export async function refresh() {
  const t = activeTab();
  if (!t || !app.path) return;
  if (t.pending) return; // the navigation in flight lists fresh contents anyway
  if (isVirtual(app.path)) {
    const path = app.path;
    const scroll = hooks.getScroll?.() ?? 0;
    await loadVirtual(path);
    if (app.path === path) restoreScroll(scroll);
    return;
  }
  if (app.error) {
    // Retry in refresh mode, so a repeat failure stays in place, not in the message bar too.
    await navigate(app.path, false, { refresh: true });
    return;
  }
  if (app.search) {
    if (!app.searching) await startSearch(app.search.query, true);
    return;
  }
  await navigate(app.path, false, { refresh: true });
}

/** Folders whose subfolder list changed; the nav tree reloads them. */
export const treeDirty = $state<{ path: string; n: number }>({ path: "", n: 0 });

/// A watcher event: refresh only if it is for what this tab is showing.
export function onFsChanged(path: string) {
  treeDirty.path = path;
  treeDirty.n++;
  if (samePath(path, app.path) && !app.search) refresh();
}

export function back() {
  const t = activeTab();
  if (t && t.histIdx > 0) {
    const from = t.hist[t.histIdx];
    t.histIdx--;
    syncNav();
    navigate(t.hist[t.histIdx], false, upFocus(from, t.hist[t.histIdx]));
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

/// Coming back up out of a child folder selects that child, like Explorer.
function upFocus(from: string, to: string): NavOpts {
  const parent = parentPath(from);
  return parent && samePath(parent, to) ? { select: [baseName(from)] } : {};
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

export async function newTab(path?: string) {
  // Save the outgoing tab's selection before the new tab's listing replaces it.
  await leaveTab();
  const id = nextTabId++;
  tabs.push({ id, path: "", hist: [], histIdx: -1, scroll: 0, pending: 0, query: "", keepKeys: [], keepAll: false });
  app.activeTab = id;
  // New tabs open on the configured start page; a start folder that has since
  // been deleted or unmounted falls back to Home instead of an error page.
  const target = path ?? (settings.startPage || HOME);
  const ok = await navigate(target);
  if (!ok && tabs.find((t) => t.id === id)?.path === "") {
    const why = app.status;
    await navigate(HOME, false);
    if (!path) app.status = `The start folder couldn't be opened, so this tab opened Home. ${why}`;
  }
}

/// Remember what the outgoing tab shows (scroll, selection, search) so it can be
/// listed again exactly when reselected.
async function leaveTab() {
  const t = activeTab();
  if (!t) return;
  saveScroll();
  t.query = app.search ? app.search.query : "";
  t.keepAll = app.allSelected;
  t.keepKeys = [];
  if (isShellLocation(app.path)) t.keepKeys = [...shellSelection.paths];
  else if (!app.allSelected && sel.size && !isVirtual(app.path)) {
    t.keepKeys = await withSel((sel) => invoke<string[]>("sel_keys", { sel })).catch(() => []);
  }
}

export async function switchTab(id: number) {
  if (id === app.activeTab) return;
  const t = tabs.find((t) => t.id === id);
  if (!t) return;
  const leaving = leaveTab();
  app.activeTab = id;
  syncNav();
  await leaving;
  await showTab(t);
}

async function showTab(t: Tab) {
  // Read before navigating: a committed navigation clears the tab's search.
  const { query, keepAll, scroll } = t;
  // ponytail: re-list on switch (fast) instead of caching per-tab listings
  const ok = await navigate(t.path, false, { select: t.keepKeys });
  if (!ok && t.id === app.activeTab && !t.pending) {
    // Its folder is gone (deleted, unplugged) since the tab last showed it.
    // The tab moves to the nearest folder that still opens; leaving the
    // previous tab's items on screen would put them under this tab's name.
    await openNearest(t.path, app.status, true);
    return;
  }
  if (!ok || t.id !== app.activeTab) return;
  if (keepAll && !isVirtual(t.path)) selectAll();
  if (query) {
    app.filter = query;
    await startSearch(query, true);
  }
  restoreScroll(scroll);
}

// ---- Reopen tabs from last time ----
const SESSION = "session";
const SESSION_V = 1;
/** At most this many tabs come back, so a damaged record can't flood the window. */
const SESSION_MAX = 50;

/// Remember this window's tabs for "Reopen tabs from last time". Every window
/// writes it as its tabs change, so the window used last wins.
export function saveSession() {
  const paths = tabs.map((t) => t.path);
  if (!paths.length || paths.some((p) => !p)) return; // a tab is still opening
  const active = Math.max(0, tabs.findIndex((t) => t.id === app.activeTab));
  try {
    localStorage.setItem(SESSION, JSON.stringify({ v: SESSION_V, tabs: paths, active }));
  } catch {
    // Storage full or unavailable: the next start just opens a new tab.
  }
}

/// Bring back the tabs saveSession kept: each one's location, and which was
/// active. Only that one is listed now; the others list when shown, and one
/// whose folder is gone opens the nearest folder that still exists (showTab).
/// False when there's no usable record, so the caller opens a new tab.
export async function restoreSession(): Promise<boolean> {
  let s: { v?: unknown; tabs?: unknown; active?: unknown } | null = null;
  try {
    s = JSON.parse(localStorage.getItem(SESSION) ?? "null");
  } catch {
    return false;
  }
  const paths = s?.tabs;
  if (s?.v !== SESSION_V || !Array.isArray(paths) || !paths.length || !paths.every((p) => typeof p === "string" && p)) return false;
  const list = (paths as string[]).slice(0, SESSION_MAX);
  const made = list.map((p) => ({ id: nextTabId++, path: p, hist: [p], histIdx: 0, scroll: 0, pending: 0, query: "", keepKeys: [], keepAll: false }));
  tabs.push(...made);
  const at = typeof s.active === "number" && s.active >= 0 && s.active < made.length ? s.active : 0;
  app.activeTab = made[at].id;
  syncNav();
  await showTab(tabs.find((t) => t.id === made[at].id)!);
  return true;
}

export function closeTab(id: number) {
  const idx = tabs.findIndex((t) => t.id === id);
  if (idx < 0) return;
  if (tabs[idx].path) closedTabs.push(tabs[idx].path);
  tabs.splice(idx, 1);
  if (!tabs.length) {
    getCurrentWindow().close();
    return;
  }
  if (app.activeTab === id) {
    const next = tabs[Math.min(idx, tabs.length - 1)];
    app.activeTab = next.id;
    syncNav();
    showTab(next);
  }
}
/// Move a tab to a window of its own: a new window opens its location (a new
/// Filezy process, as Ctrl+N makes) and the tab closes here. Its history
/// stays behind, as when Explorer opens a folder in a new window.
export async function moveTabToWindow(id: number) {
  await transferTab(id);
}

/** Snapshot the tab's navigation and selection before handing it to a window. */
export async function exportTab(id: number) {
  if (id === app.activeTab) await leaveTab();
  const tab = tabs.find((t) => t.id === id);
  return tab ? JSON.parse(JSON.stringify(tab)) as Tab : null;
}

/** Only accept bounded, version-independent data from another Filezy window. */
export async function importTab(value: unknown) {
  const t = value as Tab | null;
  if (!t || typeof t.path !== "string" || !t.path || !Array.isArray(t.hist) || !t.hist.length ||
      t.hist.length > 1000 || !t.hist.every((p) => typeof p === "string") ||
      !Number.isInteger(t.histIdx) || t.histIdx < 0 || t.histIdx >= t.hist.length ||
      typeof t.query !== "string" || !Array.isArray(t.keepKeys) || !t.keepKeys.every((k) => typeof k === "string") ||
      !Number.isFinite(t.scroll) || typeof t.keepAll !== "boolean") throw new Error("Invalid tab record.");
  await leaveTab();
  const tab: Tab = { id: nextTabId++, path: t.path, hist: [...t.hist], histIdx: t.histIdx,
    scroll: Math.max(0, t.scroll), query: t.query, keepKeys: [...t.keepKeys], keepAll: t.keepAll, pending: 0 };
  tabs.push(tab); app.activeTab = tab.id; syncNav();
  await showTab(tab);
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
  if (isShellLocation(app.path)) {
    return void navigate(app.path === DEVICES || !shellFolder.parent ? THIS_PC : shellLocation(shellFolder.parent));
  }
  if (isVirtual(app.path)) return;
  const from = app.search?.root ?? app.path;
  const parent = parentPath(from);
  // Explorer: Up from a drive root lands on This PC.
  if (!parent) {
    navigate(THIS_PC);
    return;
  }
  navigate(parent, true, { select: [baseName(from)] });
}

// ---- sorting and grouping ----

interface Arranged {
  rev: number;
  total: number;
  groups: Group[];
  kept: Kept;
  notice: string | null;
}

// Re-sorts in flight; the E2E hook counts them as unsettled listing work.
let arranging = 0;
/** No navigation, search, re-sort or row fetch is outstanding (for the E2E hook). */
export function listingIdle(): boolean {
  return !activeTab()?.pending && !app.loading && !app.searching && !arranging && pending.size === 0;
}

/// Wait while `busy` holds, for at most `ms`.
async function settle(busy: () => unknown, ms: number) {
  for (const end = Date.now() + ms; busy() && Date.now() < end; ) await new Promise((r) => setTimeout(r, 20));
}

/// Wait (briefly) until no re-list or re-sort of the active tab is in flight.
const listingSettled = () => settle(() => activeTab()?.pending || arranging, 3000);

/// Run a command on the selection. A folder that changes often (a download in
/// progress) re-lists under the user; wait out a re-list in flight, and if the
/// listing still changed underneath, retry once with the selection as re-mapped
/// onto the new listing.
export async function withSel<T>(cmd: (sel: Keep) => Promise<T>): Promise<T> {
  await listingSettled();
  try {
    return await cmd(selRef());
  } catch (e) {
    if (!String(e).startsWith("The folder changed")) throw e;
    await listingSettled();
    return await cmd(selRef());
  }
}

/// Apply the current sort/group to the backend listing, keeping the selection.
async function arrangeNow(scrollTop: boolean) {
  const keep = keepNow();
  arranging++;
  try {
    const res = await invoke<Arranged>("arrange", {
      req: {
        rev: app.rev,
        arr: { sort: app.sortKey, asc: app.asc, group: app.groupKey, tzOffMin: tzOffsetMin() },
        keep,
      },
    });
    const pre = await prefetch(res.rev, res.total, !scrollTop);
    app.rev = res.rev;
    app.groups = res.groups;
    if (res.notice) app.status = res.notice;
    installRows(res.total, pre);
    restoreKept(res.kept, keep, res.total);
  } catch (e) {
    // The listing moved on underneath (a refresh or new search); that one rules.
    if (!String(e).startsWith("The folder changed")) app.status = String(e);
  } finally {
    arranging--;
  }
  if (scrollTop) hooks.scrollTo?.(0);
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
  if (!app.search) savePrefs();
  await arrangeNow(false);
}

export async function setGroup(key: string) {
  app.groupKey = key;
  groupAuto = false;
  collapsedGroups.clear();
  if (!app.search) savePrefs();
  await arrangeNow(true);
}

// ---- search ----

let filterTimer: ReturnType<typeof setTimeout> | undefined;
export function setFilter(q: string) {
  if (isVirtual(app.path)) return; // virtual pages have no listing to search
  app.filter = q;
  clearTimeout(filterTimer);
  filterTimer = setTimeout(() => startSearch(q), 300);
}

/// Search options: set or clear one filter in the query, and search again.
export function applyFilter(key: string, value: string | null) {
  const q = value === null && key === "*" ? withoutFilters(app.filter) : withFilter(app.filter, key, value);
  app.filter = q;
  startSearch(q, true);
}

/// Search options > location. This folder and All subfolders are remembered,
/// as in Explorer; a drive or every drive is for the search at hand.
export function setSearchScope(scope: SearchScope) {
  app.searchScope = scope;
  if (scope === "folder" || scope === "subfolders") setSetting("searchScope", scope);
  if (app.filter.trim()) startSearch(app.filter, true);
}

/// This drive and All drives last one search: leaving it (Escape, another
/// folder) goes back to the scope saved in Settings.
function endWideScope() {
  if (app.searchScope === "drive" || app.searchScope === "all") app.searchScope = settings.searchScope as SearchScope;
}

/// Recent searches keep what the user settled on (Enter, leaving the search
/// box, a menu pick), not every pause while typing.
export function rememberSearch(q: string) {
  q = q.trim();
  if (!q) return;
  const list = [q, ...settings.searchHistory.filter((x) => x.toLowerCase() !== q.toLowerCase())].slice(0, 20);
  setSetting("searchHistory", list);
}

export function clearSearchHistory() {
  setSetting("searchHistory", []);
}

/// List matches for `q` under the folder (or the current search's root).
/// An empty query goes back to the folder itself.
export async function startSearch(q: string, again = false) {
  clearTimeout(filterTimer);
  const t = activeTab();
  if (!t) return;
  const query = q.trim();
  // Typed while a folder is still opening: search that folder once it's open,
  // not the one being left.
  if (query) await settle(() => app.loading, 15_000);
  if (t.id !== app.activeTab) return;
  const root = app.search?.root ?? app.path;
  if (!query) {
    endWideScope();
    if (app.search) {
      app.search = null;
      app.searching = false;
      await navigate(root, false);
    }
    return;
  }
  if (!again && app.search && app.search.query === query) return;
  const seq = seqCounter >= 0 ? ++seqCounter : await firstSeq();
  t.pending = seq;
  // This supersedes a navigation that is still loading; its strip goes too.
  app.loading = false;
  let res: { rev: number; root: string };
  try {
    res = await invoke("search_start", {
      req: {
        seq,
        root,
        query,
        showHidden: settings.showHidden,
        showProtected: settings.showProtected,
        scope: app.searchScope,
        tzOffMin: tzOffsetMin(),
      },
    });
  } catch (e) {
    if (t.pending === seq) {
      t.pending = 0;
      if (String(e) !== "superseded") app.status = String(e);
    }
    return;
  }
  if (t.pending !== seq || t.id !== app.activeTab) return;
  t.pending = 0;
  t.query = query;
  if (again) rememberSearch(query);
  app.search = { seq, root: res.root, query, done: false, stopped: false, capped: false, skipped: 0, provider: "", note: "" };
  app.searching = true;
  app.rev = res.rev;
  app.groups = [];
  app.status = "";
  clearSel();
  app.renaming = -1;
  resetRows(0);
  hooks.scrollTo?.(0);
  const early = earlyUpdate;
  earlyUpdate = null;
  if (early?.seq === seq) searchUpdate(early);
}

/// Escape in the search box: back to the folder now, not after the typing delay.
export function clearSearch() {
  app.filter = "";
  startSearch("");
}

export function stopSearch() {
  if (app.search && app.searching) invoke("search_stop", { seq: app.search.seq }).catch(() => {});
}

interface SearchUpdate {
  seq: number;
  rev: number;
  total: number;
  done: boolean;
  skipped: number;
  capped: boolean;
  stopped: boolean;
  provider: string;
  note: string | null;
}

// A small search can finish before search_start's reply arrives; its update
// waits here until startSearch has set up app.search.
let earlyUpdate: SearchUpdate | null = null;

/// Called from the "search-update" event. Results are append-only within one
/// revision, so cached rows keep their indices and only the row array grows.
export function searchUpdate(u: SearchUpdate) {
  const s = app.search;
  if (!s || u.seq !== s.seq) {
    if (activeTab()?.pending === u.seq) earlyUpdate = u;
    return;
  }
  if (u.rev !== app.rev) return;
  app.total = u.total;
  rows.length = u.total;
  s.skipped = u.skipped;
  s.capped = u.capped;
  s.provider = u.provider;
  s.note = u.note ?? "";
  if (u.done) {
    s.done = true;
    s.stopped = u.stopped;
    app.searching = false;
    // Discovery order while streaming; the chosen sort once complete.
    arrangeNow(false);
  }
}

// ---- rows ----

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
    invoke<Entry[]>("get_range", { rev: app.rev, start: cStart, count: CHUNK })
      .then((got) => {
        if (app.version !== v) return;
        for (let i = 0; i < got.length; i++) rows[cStart + i] = got[i];
      })
      .catch(() => {}) // stale revision: a newer listing is on its way
      .finally(() => pending.delete(key));
  }
}

// exe/lnk/ico can have unique per-file icons; everything else shares by extension.
const PATH_ICON_EXTS = new Set(["exe", "lnk", "ico", "url"]);

export function requestMeta(e: Entry) {
  const key = e.is_dir ? "//dir" : e.ext;
  if (!extMeta.has(key)) {
    extMeta.set(key, { icon: null, type_name: "" });
    invoke<ExtMeta>("ext_meta", { ext: e.is_dir ? "" : e.ext, isDir: e.is_dir })
      .then((m) => extMeta.set(key, m))
      .catch(() => {});
  }
  if (!e.is_dir && PATH_ICON_EXTS.has(e.ext)) {
    const p = entryPath(e);
    const key = `${p}|${e.modified}`;
    if (!pathIcons.has(key)) {
      if (pathIcons.size > 2000) pathIcons.clear(); // ponytail: dumb cap; LRU if it ever matters
      pathIcons.set(key, null);
      invoke<string | null>("path_icon", { path: p })
        .then((ic) => pathIcons.set(key, ic))
        .catch(() => {});
    }
  }
}

export function iconOf(e: Entry): string | null {
  if (!e.is_dir && PATH_ICON_EXTS.has(e.ext)) {
    const ic = pathIcons.get(`${entryPath(e)}|${e.modified}`);
    if (ic) return ic;
  }
  return extMeta.get(e.is_dir ? "//dir" : e.ext)?.icon ?? null;
}

function thumbKey(e: Entry, size: number): string {
  return `${entryPath(e)}|${size}|${e.modified}`;
}

export function requestThumb(e: Entry, size: number) {
  const key = thumbKey(e, size);
  // An entry inside an archive has no file to draw a thumbnail from.
  if (thumbs.has(key) || isArchive(app.path)) return;
  if (thumbs.size > 700) thumbs.clear(); // ponytail: dumb cap; LRU if memory matters
  thumbs.set(key, null);
  invoke<string | null>("thumb", { path: entryPath(e), size }).then((t) => {
    if (t) thumbs.set(key, t);
  });
}

export function thumbOf(e: Entry, size: number): string | null {
  return thumbs.get(thumbKey(e, size)) ?? iconOf(e);
}

/** Pictures and videos, whose thumbnails are photos rather than icons. */
const PHOTO_EXTS = /^(jpe?g|jfif|png|gif|bmp|tiff?|webp|hei[cf]|avif|raw|cr2|nef|dng|arw|mp4|m4v|mov|mkv|avi|wmv|webm)$/;

/// Whether the row shows a photo-like thumbnail, which Gallery crops to fill
/// its square; anything else (an icon) is shown whole.
export function isPhotoThumb(e: Entry, size: number): boolean {
  return !e.is_dir && PHOTO_EXTS.test(e.ext) && !!thumbs.get(thumbKey(e, size));
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
  const f = locale.f;
  if (f?.date) return `${formatWith(f.date, d, f)} ${formatWith(f.time, d, f)}`;
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
  else openPath(p);
}

/// Open a file as a double-click does. An archive that Windows' own zip
/// folder opens, or one inside another archive, is shown as a folder instead
/// (in a new tab with `inNewTab`).
export async function openPath(path: string, inNewTab = false) {
  try {
    const at = await invoke<string | null>("open_path", { path });
    if (at) await (inNewTab ? newTab(at) : navigate(at));
  } catch (err) {
    app.status = String(err);
  }
}

export async function refreshClipState() {
  const st = await invoke<{ files: boolean; cut: string[] }>("clip_state").catch(() => ({ files: false, cut: [] }));
  app.canPaste = st.files;
  if (st.cut.length || app.cut.size) app.cut = new Set(st.cut.map(pathKey));
}

/// Cut items look ghosted until they are pasted or the clipboard changes, as in Explorer.
export function isCut(e: Entry): boolean {
  return app.cut.size > 0 && app.cut.has(pathKey(entryPath(e)));
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
  invoke<ExtMeta>("ext_meta", { ext, isDir: false })
    .then((m) => extMeta.set(ext, m))
    .catch(() => {});
}

export function selectAll() {
  if (!can("select")) return;
  sel.clear();
  app.allSelected = true;
}
