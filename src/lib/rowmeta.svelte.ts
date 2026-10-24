// What the file views show about each row beyond the listing itself: icons,
// type names, thumbnails, property columns and cloud status, fetched on demand
// and cached, plus the shared date and size formats and the New menu's types.
import { invoke } from "@tauri-apps/api/core";
import { SvelteMap } from "svelte/reactivity";
import { app, entryPath, type Entry } from "./store.svelte";
import { isArchive } from "./paths";
import { formatWith, type LocaleFormats } from "./datefmt";

export interface ExtMeta {
  icon: string | null;
  type_name: string;
}
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

/** Device pixels for a thumbnail drawn `size` CSS pixels wide, as the shell wants. */
export const devicePx = (size: number) => Math.min(1024, Math.round(size * (window.devicePixelRatio || 1)));

function thumbKey(e: Entry, size: number): string {
  return `${entryPath(e)}|${devicePx(size)}|${e.modified}`;
}

export function requestThumb(e: Entry, size: number) {
  const key = thumbKey(e, size);
  // An entry inside an archive has no file to draw a thumbnail from.
  if (thumbs.has(key) || isArchive(app.path)) return;
  if (thumbs.size > 700) thumbs.clear(); // ponytail: dumb cap; LRU if memory matters
  thumbs.set(key, null);
  invoke<string | null>("thumb", { path: entryPath(e), px: devicePx(size) })
    .then((t) => {
      if (t) thumbs.set(key, t);
    })
    .catch(() => {});
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

export interface NewItem {
  ext: string;
  name: string;
}
export const newItems = $state<{ list: NewItem[] | null }>({ list: null });

export async function loadNewItems() {
  if (newItems.list) return;
  const list = await invoke<NewItem[]>("new_menu_items").catch(() => []);
  // Two types can share a name (an app registering .cbv and .cbh both as
  // "ChessBase Database"); those say which extension each one makes.
  const shared = (it: NewItem) => list.filter((x) => x.name === it.name).length > 1;
  newItems.list = list.map((it) => (shared(it) ? { ...it, name: `${it.name} (.${it.ext})` } : it));
  for (const it of newItems.list ?? []) requestExtMeta(it.ext);
}

export function requestExtMeta(ext: string) {
  if (extMeta.has(ext)) return;
  extMeta.set(ext, { icon: null, type_name: "" });
  invoke<ExtMeta>("ext_meta", { ext, isDir: false })
    .then((m) => extMeta.set(ext, m))
    .catch(() => {});
}
