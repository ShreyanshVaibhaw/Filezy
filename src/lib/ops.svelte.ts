// File operations from the UI: what each one actually did (per item), the
// activity list with retry, and undo/redo built only from verified outcomes.
import { invoke } from "@tauri-apps/api/core";
import {
  app, rowAt, selCount, firstSel, can, refresh, refreshClipState, entryPath, navigate, say, withSel,
  addTabs, pinAdd, pinRemove, selectKeys,
} from "./store.svelte";
import { settings } from "./settings.svelte";
import { confirm } from "./ask.svelte";
import { baseName, isArchive, isLibrary, parentPath, samePath, pathKey } from "./paths";

export type ItemState = "done" | "skipped" | "failed" | "unattempted";
export interface OpItem {
  src: string;
  dst: string;
  state: ItemState;
  error: string | null;
  replaced: boolean;
  bin_id: string | null;
  size: number;
  modified: number;
  /** The shell's HRESULT for the item; negative for a failure. */
  hr: number;
}
export interface OpResult {
  kind: "copy" | "move" | "recycle" | "delete";
  items: OpItem[];
  cancelled: boolean;
  error: string | null;
  /** Cut-paste only: "cleared", "rewritten", or "changed elsewhere". */
  clipboard: string | null;
}
interface Stamp {
  exists: boolean;
  size: number;
  modified: number;
}

export const plural = (n: number, word = "item") => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

// ---- activity: what recent operations did, with retry for what didn't work ----

export interface Activity {
  id: number;
  title: string;
  /** ok: recorded only; info: shown briefly; warn and error: shown until dismissed. */
  tone: "ok" | "info" | "warn" | "error";
  problems: OpItem[];
  retry?: () => Promise<void>;
  /** The full result, for the activity details and tests. */
  result: OpResult;
}
export const activity = $state<{ list: Activity[]; open: boolean; current: Activity | null }>({
  list: [],
  open: false,
  current: null,
});
let activitySeq = 0;

const WORDS: Record<OpResult["kind"], [string, string]> = {
  copy: ["Copied", "copy"],
  move: ["Moved", "move"],
  recycle: ["Deleted", "delete"],
  delete: ["Permanently deleted", "delete"],
};

/// One line on what an operation actually did, in Explorer's words.
export function summarize(r: OpResult): { title: string; tone: Activity["tone"] } {
  const n = r.items.length;
  const count = (s: ItemState) => r.items.filter((i) => i.state === s).length;
  const [done, skipped, failed, unattempted] = [count("done"), count("skipped"), count("failed"), count("unattempted")];
  const [past, verb] = WORDS[r.kind];
  if (r.error && !done) return { title: `Couldn't ${verb} ${plural(n)}. ${r.error}`, tone: "error" };
  if (done === n) return { title: `${past} ${plural(n)}`, tone: "ok" };
  if (r.cancelled) {
    if (done) return { title: `Cancelled after ${past.toLowerCase()} ${done} of ${plural(n)}.`, tone: "warn" };
    // Saying no to a delete confirmation needs no comment; a cancelled copy or
    // move (say, a file in use) is worth a brief note with Retry.
    return r.kind === "copy" || r.kind === "move"
      ? { title: `Cancelled. Nothing was ${r.kind === "copy" ? "copied" : "moved"}.`, tone: "info" }
      : { title: "Cancelled. Nothing was changed.", tone: "ok" };
  }
  if (!failed && !unattempted && skipped === n) return { title: `Skipped ${plural(n)}. Nothing was changed.`, tone: "ok" };
  const parts = [`${past} ${done} of ${plural(n)}`];
  if (skipped) parts.push(`${skipped} skipped`);
  if (failed) parts.push(`${failed} failed`);
  if (unattempted) parts.push(`${unattempted} not attempted`);
  const first = r.items.find((i) => i.state === "failed")?.error;
  return { title: parts.join(", ") + "." + (first ? ` ${first}` : ""), tone: failed || unattempted ? "error" : "warn" };
}

/// Record an outcome; problems become the visible message with Details/Retry.
function report(r: OpResult, retry?: (items: OpItem[]) => Promise<void>) {
  const s = summarize(r);
  const problems = r.items.filter((i) => i.state !== "done");
  const again = problems.filter((i) => i.state === "failed" || i.state === "unattempted");
  pushActivity({
    id: ++activitySeq,
    title: s.title,
    tone: s.tone,
    problems,
    retry: retry && again.length ? () => retry(again) : undefined,
    result: r,
  });
}

/// Keep the last 30 outcomes; anything but a plain success shows in the message bar.
function pushActivity(a: Activity) {
  activity.list.unshift(a);
  if (activity.list.length > 30) activity.list.pop();
  if (a.tone !== "ok") {
    app.status = a.title;
    activity.current = a;
  }
}

/// Record an outcome of work Filezy does itself (extracting, compressing):
/// its line, its tone, and what wasn't done, for Details.
export function note(title: string, tone: Activity["tone"], problems: OpItem[] = []) {
  const result: OpResult = { kind: "copy", items: problems, cancelled: false, error: null, clipboard: null };
  pushActivity({ id: ++activitySeq, title, tone, problems, result });
}

export function dismissNotice() {
  app.status = "";
  activity.current = null;
}

// ---- undo / redo ----
// Entries hold only what verifiably happened. Undo refuses or asks where the
// disk no longer matches the record, and keeps whatever it could not reverse.

interface UndoItem {
  from: string;
  to: string;
  binId?: string;
  size: number;
  modified: number;
  replaced: boolean;
}
interface UndoEntry {
  kind: "copy" | "move" | "rename" | "recycle" | "new";
  items: UndoItem[];
  /** Destination folder, for redoing a copy or move. */
  dest?: string;
}

export const history = $state<{ undo: UndoEntry[]; redo: UndoEntry[] }>({ undo: [], redo: [] });
const MAX_UNDO = 30;

function pushUndo(e: UndoEntry, clearRedo = true) {
  if (!e.items.length) return;
  history.undo.push(e);
  if (history.undo.length > MAX_UNDO) history.undo.shift();
  if (clearRedo) history.redo.length = 0;
}

function record(r: OpResult, dest?: string, fromRedo = false) {
  const done = r.items.filter((i) => i.state === "done");
  if (r.kind === "delete" || !done.length) return;
  pushUndo(
    {
      kind: r.kind === "recycle" ? "recycle" : r.kind,
      dest,
      items: done.map((i) => ({
        from: i.src,
        to: i.dst,
        binId: i.bin_id ?? undefined,
        size: i.size,
        modified: i.modified,
        replaced: i.replaced,
      })),
    },
    !fromRedo,
  );
}

/** Device copies use the same per-item reporting and safe undo as local ones. */
export function recordExternalCopy(result: OpResult) {
  const first = result.items.find((i) => i.state === "done" && i.dst);
  record(result, first ? parentPath(first.dst) ?? undefined : undefined);
  report(result);
}

const LABEL: Record<UndoEntry["kind"], string> = {
  copy: "copy",
  move: "move",
  rename: "rename",
  recycle: "delete",
  new: "new item",
};
const names = (items: UndoItem[], key: "from" | "to") =>
  items.slice(0, 5).map((i) => baseName(i[key])).join("\n") + (items.length > 5 ? `\n…and ${items.length - 5} more` : "");

/// Reverse one entry. Returns the items it reversed and the ones left to retry.
async function reverse(e: UndoEntry): Promise<{ undone: UndoItem[]; left: UndoItem[]; notes: string[] }> {
  const notes: string[] = [];
  if (e.kind === "copy" || e.kind === "new") {
    // Remove what the operation created, but only while it is still that.
    const lost = e.items.filter((i) => i.replaced);
    if (lost.length) notes.push(`${plural(lost.length)} replaced existing items, whose old contents can't be brought back.`);
    const live = e.items.filter((i) => !i.replaced);
    const st = await invoke<Stamp[]>("path_stamps", { paths: live.map((i) => i.to) });
    const present = live.filter((_, k) => st[k].exists);
    const changed = present.filter((i) => {
      const s = st[live.indexOf(i)];
      return s.size !== i.size || s.modified !== i.modified;
    });
    let targets = present;
    if (changed.length) {
      const go = await confirm(
        `Undo ${LABEL[e.kind]}`,
        `${plural(changed.length)} changed after ${e.kind === "new" ? "being created" : "being copied"}:\n${names(changed, "to")}\n\nMove ${changed.length === 1 ? "it" : "them"} to the Recycle Bin anyway?`,
        { ok: "Move to Recycle Bin" },
      );
      if (!go) targets = present.filter((i) => !changed.includes(i));
    }
    if (!targets.length) return { undone: [], left: e.items.filter((i) => present.includes(i) && !targets.includes(i)), notes };
    const r = await invoke<OpResult>("recycle_paths", { paths: targets.map((i) => i.to) });
    const ok = new Set(r.items.filter((i) => i.state === "done").map((i) => pathKey(i.src)));
    return {
      undone: targets.filter((i) => ok.has(pathKey(i.to))),
      left: [...targets.filter((i) => !ok.has(pathKey(i.to))), ...present.filter((i) => !targets.includes(i))],
      notes: r.items.some((i) => i.state === "failed") ? [...notes, summarize(r).title] : notes,
    };
  }
  if (e.kind === "move" || e.kind === "rename") {
    // Put items back, refusing where something new now sits at the old path.
    const st = await invoke<Stamp[]>("path_stamps", { paths: e.items.map((i) => i.from) });
    const blocked = e.items.filter((_, k) => st[k].exists);
    if (blocked.length) notes.push(`${plural(blocked.length)} can't go back: something else now has ${blocked.length === 1 ? "its" : "their"} old name.\n${names(blocked, "from")}`);
    const go = e.items.filter((_, k) => !st[k].exists);
    if (!go.length) return { undone: [], left: blocked, notes };
    const r = await invoke<OpResult>("move_back", { items: go.map((i) => [i.to, i.from]) });
    const ok = new Set(r.items.filter((i) => i.state === "done").map((i) => pathKey(i.src)));
    if (r.items.some((i) => i.state !== "done")) notes.push(summarize(r).title);
    return {
      undone: go.filter((i) => ok.has(pathKey(i.to))),
      left: [...go.filter((i) => !ok.has(pathKey(i.to))), ...blocked],
      notes,
    };
  }
  // recycle: restore exactly these items from the Recycle Bin, by their bin ids.
  const withId = e.items.filter((i) => i.binId);
  if (withId.length < e.items.length) notes.push(`${plural(e.items.length - withId.length)} were deleted without the Recycle Bin and can't be restored.`);
  if (!withId.length) return { undone: [], left: [], notes };
  try {
    await invoke<number>("recycle_restore", { ids: withId.map((i) => i.binId) });
  } catch (err) {
    notes.push(String(err));
  }
  // The bin restores asynchronously and may rename on conflict; check the disk.
  const st = await waitForStamps(withId.map((i) => i.from), (s) => s.exists);
  return {
    undone: withId.filter((_, k) => st[k].exists),
    left: withId.filter((_, k) => !st[k].exists),
    notes,
  };
}

/// Poll the disk briefly until every path satisfies `ok` (shell verbs such as
/// restore finish after they return).
async function waitForStamps(paths: string[], ok: (s: Stamp) => boolean): Promise<Stamp[]> {
  let st: Stamp[] = [];
  for (let i = 0; i < 20; i++) {
    st = await invoke<Stamp[]>("path_stamps", { paths });
    if (st.every(ok)) break;
    await new Promise((r) => setTimeout(r, 100));
  }
  return st;
}

let busyUndo = false;

async function undoImpl() {
  const e = history.undo.at(-1);
  if (!e) {
    say("Nothing to undo");
    return;
  }
  if (busyUndo) return;
  busyUndo = true;
  try {
    const { undone, left, notes } = await reverse(e);
    // By identity: an operation that finished meanwhile pushed its own entry on top.
    const at = history.undo.lastIndexOf(e);
    if (at >= 0) history.undo.splice(at, 1, ...(left.length ? [{ ...e, items: left }] : []));
    // Copies and moves can be redone; new items cannot.
    if (undone.length && e.kind !== "new") history.redo.push({ ...e, items: undone });
    const head = undone.length
      ? `Undid ${LABEL[e.kind]} of ${plural(undone.length)}.`
      : `Couldn't undo ${LABEL[e.kind]}.`;
    const text = [head, ...notes].join(" ");
    activity.current = null;
    if (undone.length && !notes.length) say(text);
    else app.status = text;
  } catch (err) {
    app.status = "Undo: " + String(err);
  } finally {
    busyUndo = false;
  }
  await refresh();
}

async function redoImpl() {
  const e = history.redo.at(-1);
  if (!e) {
    say("Nothing to redo");
    return;
  }
  if (busyUndo) return;
  busyUndo = true;
  try {
    history.redo.pop();
    let r: OpResult;
    const dest = e.dest ?? parentPath(e.items[0].to) ?? "";
    if (e.kind === "copy") r = await invoke<OpResult>("copy_into", { srcs: e.items.map((i) => i.from), dest });
    else if (e.kind === "move") r = await invoke<OpResult>("move_into", { srcs: e.items.map((i) => i.from), dest });
    else if (e.kind === "rename") r = await invoke<OpResult>("move_back", { items: e.items.map((i) => [i.from, i.to]) });
    else r = await invoke<OpResult>("recycle_paths", { paths: e.items.map((i) => i.from) });
    if (e.kind === "rename") {
      const done = r.items.filter((i) => i.state === "done");
      pushUndo({ kind: "rename", items: done.map((i) => ({ from: i.src, to: i.dst, size: i.size, modified: i.modified, replaced: false })) }, false);
    } else {
      record(r, dest, true);
    }
    const s = summarize(r);
    if (s.tone === "ok") say(`Redid ${LABEL[e.kind]} of ${plural(r.items.length)}.`);
    else app.status = s.title;
  } catch (err) {
    app.status = "Redo: " + String(err);
  } finally {
    busyUndo = false;
  }
  await refresh();
}

// ---- commands ----

export async function copySel(cut = false) {
  if (!selCount()) return;
  if (isArchive(app.path)) {
    say(cut ? "Items in an archive can't be cut. Extract them first." : "To copy items out of an archive, extract them.");
    return;
  }
  try {
    await withSel((sel) => invoke("clip_copy", { sel, cut }));
  } catch (e) {
    app.status = String(e);
  }
  await refreshClipState();
}

/// Re-list and select what was just pasted, created or renamed, as Explorer
/// does. With `rename`, a single new item opens its rename box.
export async function showResult(paths: string[], rename = false) {
  // Rows of a library or of search results carry their folder, so the backend
  // keys them by full path (listing.rs Entry::key); a folder's rows by name.
  const byPath = isLibrary(app.path) || !!app.search;
  const keys = byPath ? paths : paths.filter((p) => samePath(parentPath(p) ?? "", app.path)).map(baseName);
  if (app.search) {
    // Re-listing would put the search's folder under the search box; run the search again.
    await refresh();
    await selectKeys(keys);
  } else {
    await navigate(app.path, false, { refresh: true, select: keys.length ? keys : undefined });
  }
  if (rename && keys.length === 1 && app.focus >= 0) app.renaming = app.focus;
}

/// Paste whatever files are on the clipboard right now. The clipboard is read
/// at paste time, not from the cached canPaste flag, so a fast Ctrl+X, Ctrl+V
/// (or a copy in another app a moment ago) always works.
async function pasteImpl() {
  if (!can("paste")) return;
  const dest = app.path;
  let r: OpResult;
  try {
    r = await invoke<OpResult>("paste", { dest });
  } catch (e) {
    app.status = String(e);
    return;
  }
  if (!r.items.length) return refreshClipState(); // nothing to paste, like Explorer
  const retry = async (items: OpItem[]) => {
    const again = await invoke<OpResult>(r.kind === "move" ? "move_into" : "copy_into", { srcs: items.map((i) => i.src), dest });
    report(again);
    record(again, dest);
    await refresh();
  };
  report(r, retry);
  record(r, dest);
  await showResult(r.items.filter((i) => i.state === "done").map((i) => i.dst));
  refreshClipState();
}

export type DropOp = "copy" | "move" | "link" | "recycle";

/// A drop from any window, Filezy's own included, onto a folder or the Recycle
/// Bin. The drop target already chose the operation the pointer showed.
async function dropOnImpl(paths: string[], dest: string, op: DropOp) {
  try {
    if (op === "link") {
      const made = await invoke<string[]>("make_links", { paths, dest });
      if (made.length) pushUndo({ kind: "new", items: await Promise.all(made.map(stampNew)) });
      return void (await showResult(made));
    }
    const run = (srcs: string[]) =>
      op === "recycle"
        ? invoke<OpResult>("recycle_paths", { paths: srcs })
        : invoke<OpResult>("drop_paths", { paths: srcs, dest, op });
    const target = op === "recycle" ? undefined : dest;
    const r = await run(paths);
    report(r, async (items) => {
      const again = await run(items.map((i) => i.src));
      report(again);
      record(again, target);
      await refresh();
    });
    record(r, target);
    await showResult(r.items.filter((i) => i.state === "done").map((i) => i.dst));
  } catch (e) {
    app.status = String(e);
  }
}

/// Delete the selection: to the Recycle Bin, or for good with Shift+Delete.
async function delImpl(permanent = false) {
  if (!selCount() || !can("change")) return;
  let r: OpResult;
  try {
    r = await withSel((sel) => invoke<OpResult>("delete_items", { sel, permanent }));
  } catch (e) {
    app.status = String(e);
    return;
  }
  report(r, async (items) => {
    // Retry what the user asked for: after Shift+Delete, delete for good again.
    const again = await invoke<OpResult>(permanent ? "delete_paths" : "recycle_paths", { paths: items.map((i) => i.src) });
    report(again);
    record(again);
    await refresh();
  });
  record(r);
  await refresh();
}

/// Undo for items an extraction or compression made: they go to the Recycle
/// Bin, after a question if they changed since.
export function recordMade(made: { path: string; size: number; modified: number; replaced: boolean }[]) {
  pushUndo({ kind: "new", items: made.map((m) => ({ from: "", to: m.path, size: m.size, modified: m.modified, replaced: m.replaced })) });
}

async function stampNew(path: string): Promise<UndoItem> {
  const [s] = await invoke<Stamp[]>("path_stamps", { paths: [path] });
  return { from: "", to: path, size: s?.size ?? 0, modified: s?.modified ?? 0, replaced: false };
}

async function newFolderImpl() {
  if (!can("new")) return;
  try {
    const made = await invoke<string>("create_folder", { dir: app.path });
    pushUndo({ kind: "new", items: [await stampNew(made)] });
    await showResult([made], true);
  } catch (e) {
    app.status = String(e);
  }
}

async function newFileImpl(ext: string) {
  if (!can("new")) return;
  try {
    const made = await invoke<string | null>("create_new", { dir: app.path, ext });
    if (!made) return; // command-driven type: a wizard was launched instead
    pushUndo({ kind: "new", items: [await stampNew(made)] });
    await showResult([made], true);
  } catch (e) {
    app.status = String(e);
  }
}

/// Finish an inline rename. With extensions hidden, the box held only the base
/// name and the extension is kept, as in Explorer.
async function commitRenameImpl(index: number, typed: string) {
  app.renaming = -1;
  const e = rowAt(index);
  if (!e || !typed.trim()) return;
  const keepExt = !settings.showExt && !e.is_dir && e.ext;
  const newName = keepExt ? `${typed.trim()}.${e.ext}` : typed;
  if (newName === e.name) return;
  const oldExt = e.is_dir ? "" : (e.name.includes(".") ? e.name.slice(e.name.lastIndexOf(".") + 1) : "");
  const newExt = e.is_dir ? "" : (newName.trim().includes(".") ? newName.trim().slice(newName.trim().lastIndexOf(".") + 1) : "");
  if (!keepExt && !e.is_dir && oldExt.toLowerCase() !== newExt.toLowerCase()) {
    const go = await confirm(
      "Rename",
      "If you change a file name extension, the file might become unusable.\n\nAre you sure you want to change it?",
      { ok: "Yes", cancel: "No" },
    );
    if (!go) return;
  }
  try {
    const oldPath = entryPath(e);
    const newPath = await invoke<string>("rename_item", { path: oldPath, newName });
    if (newPath !== oldPath) {
      const [s] = await invoke<Stamp[]>("path_stamps", { paths: [newPath] });
      pushUndo({ kind: "rename", items: [{ from: oldPath, to: newPath, size: s?.size ?? 0, modified: s?.modified ?? 0, replaced: false }] });
    }
    await showResult([newPath]);
  } catch (err) {
    app.status = String(err);
  }
}

/// F2: rename in place for one item; several items open the batch rename.
export async function startRename() {
  if (!can("change")) return;
  if (selCount() === 1) {
    app.renaming = firstSel() ?? -1;
    return;
  }
  if (selCount() > 1) await openBatchRename();
}

// ---- batch rename ----
// Explorer's multi-item rename ("Name (1)", "Name (2)"... in list order, each
// keeping its own extension), plus find/replace and template rules. A preview
// shows the result, and names that would collide with other items, before
// anything changes.

export interface BatchItem {
  path: string;
  is_dir: boolean;
}
class Batch {
  /** Replaced whole for each batch, so raw: the items never become proxies. */
  items = $state.raw<BatchItem[]>([]);
  open = $state(false);
}
export const batch = new Batch();

export async function openBatchRename() {
  const items = await withSel((sel) => invoke<BatchItem[]>("sel_items", { sel })).catch((e) => {
    app.status = String(e);
    return [] as BatchItem[];
  });
  if (items.length < 2) return;
  batch.items = items;
  batch.open = true;
}

/// Rename the batch to `names` (renamerules.ts), in list order.
async function commitBatchImpl(names: string[]) {
  const items = batch.items;
  batch.open = false;
  let res: { from: string; to: string; error: string | null }[];
  try {
    res = await invoke("rename_many", { items: items.map((it, i) => [it.path, names[i]]) });
  } catch (e) {
    app.status = String(e);
    return;
  }
  const done = res.filter((r) => !r.error && r.to && r.to !== r.from);
  if (done.length) {
    const st = await invoke<Stamp[]>("path_stamps", { paths: done.map((r) => r.to) });
    pushUndo({
      kind: "rename",
      items: done.map((r, k) => ({ from: r.from, to: r.to, size: st[k]?.size ?? 0, modified: st[k]?.modified ?? 0, replaced: false })),
    });
  }
  const failed = res.filter((r) => r.error);
  if (failed.length) {
    app.status = `Renamed ${done.length} of ${res.length} items. ${baseName(failed[0].from)}: ${failed[0].error}`;
  }
  await showResult(done.map((r) => r.to));
}

// ---- more item commands ----

/// Open the selected folders in new tabs (Explorer's "Open in new tab").
export async function openInNewTabs() {
  const items = await withSel((sel) => invoke<BatchItem[]>("sel_items", { sel })).catch(() => [] as BatchItem[]);
  await addTabs(items.filter((it) => it.is_dir).map((it) => it.path));
}

export async function openInNewWindow() {
  const items = await withSel((sel) => invoke<BatchItem[]>("sel_items", { sel })).catch(() => [] as BatchItem[]);
  const dir = items.find((it) => it.is_dir);
  if (dir) invoke("new_window", { path: dir.path }).catch((e) => (app.status = String(e)));
}

/// Search results and Recent files: go to the item's folder with it selected.
export async function openFileLocation(path: string) {
  const parent = parentPath(path);
  if (parent) await navigate(parent, true, { select: [baseName(path)] });
}

async function createShortcutsImpl() {
  try {
    const made = await withSel((sel) => invoke<string[]>("create_shortcuts", { sel }));
    if (made.length) {
      pushUndo({ kind: "new", items: await Promise.all(made.map(stampNew)) });
      await showResult(made);
    }
  } catch (e) {
    app.status = String(e);
  }
}

async function pasteShortcutsImpl() {
  if (!can("paste")) return;
  try {
    const made = await invoke<string[]>("paste_shortcuts", { dest: app.path });
    if (made.length) {
      pushUndo({ kind: "new", items: await Promise.all(made.map(stampNew)) });
      await showResult(made);
    }
  } catch (e) {
    app.status = String(e);
  }
}

/// Windows' Share sheet. Folders can't be shared, as in Explorer.
export async function shareSel() {
  if (!can("change")) return;
  try {
    await withSel((sel) => invoke("share", { sel }));
  } catch (e) {
    app.status = String(e);
  }
}

/// Properties for the whole selection: one sheet, or Windows' combined sheet.
export async function propertiesSel() {
  if (isArchive(app.path)) return;
  try {
    await withSel((sel) => invoke("sel_properties", { sel }));
  } catch (e) {
    app.status = String(e);
  }
}

/// Copy to / Move to another window's folder (the dual-pane workflow).
export async function sendSelTo(dest: string, move: boolean) {
  if (move ? !can("change") : isArchive(app.path)) return;
  const items = await withSel((sel) => invoke<BatchItem[]>("sel_items", { sel })).catch((e) => ((app.status = String(e)), [] as BatchItem[]));
  if (items.length) await dropOn(items.map((it) => it.path), dest, move ? "move" : "copy");
}

/// Pin (or unpin) every selected folder, not just the first.
export async function pinSelected(pin: boolean) {
  if (!can("change")) return;
  const items = await withSel((sel) => invoke<BatchItem[]>("sel_items", { sel })).catch(() => [] as BatchItem[]);
  for (const it of items) if (it.is_dir) await (pin ? pinAdd(it.path) : pinRemove(it.path));
}

// ---- in-flight tracking (the E2E hook waits for these to settle) ----

let inFlight = 0;
export function opsBusy(): boolean {
  return inFlight > 0;
}
function tracked<A extends unknown[], R>(f: (...a: A) => Promise<R>): (...a: A) => Promise<R> {
  return async (...a: A) => {
    inFlight++;
    try {
      return await f(...a);
    } finally {
      inFlight--;
    }
  };
}
export const undo = tracked(undoImpl);
export const redo = tracked(redoImpl);
export const paste = tracked(pasteImpl);
export const dropOn = tracked(dropOnImpl);
export const del = tracked(delImpl);
export const newFolder = tracked(newFolderImpl);
export const newFile = tracked(newFileImpl);
export const commitRename = tracked(commitRenameImpl);
export const commitBatch = tracked(commitBatchImpl);
export const createShortcuts = tracked(createShortcutsImpl);
export const pasteShortcuts = tracked(pasteShortcutsImpl);
