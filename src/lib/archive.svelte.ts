// Archives in the UI: Extract all (Explorer's dialog: a destination, Browse,
// and "Show extracted files when complete"), extracting what's selected inside
// an archive, and Compress to ZIP file. archive.rs does the work.
import { invoke } from "@tauri-apps/api/core";
import { app, rows, firstSel, selCount, entryPath, newTab, refresh, withSel } from "./store.svelte";
import { focusedRow } from "./selection.svelte";
import { baseName, isArchive, isArchiveName, isVirtual, isWithin, joinPath, parentPath, splitArchive } from "./paths";
import { note, recordMade, showResult, type OpItem } from "./ops.svelte";

interface Made {
  path: string;
  size: number;
  modified: number;
  replaced: boolean;
}
interface Extracted {
  dest: string;
  made: Made[];
  skipped: { name: string; why: string }[];
  renamed: number;
  kept: number;
  cancelled: boolean;
  error: string | null;
}
interface Compressed {
  made: Made | null;
  cancelled: boolean;
  error: string | null;
}

export const extractDlg = $state({
  open: false,
  title: "",
  file: "",
  /** The folder inside the archive the items were picked in. */
  base: "",
  /** Inner paths; [""] is everything. */
  picked: [] as string[],
  dest: "",
  show: true,
});

const plural = (n: number, word = "item") => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

/// Where Extract all suggests: a folder named after the archive, beside it.
function defaultDest(file: string): string {
  return joinPath(parentPath(file) ?? "", baseName(file).replace(/\.(tar\.gz|tgz|zip|7z|tar)$/i, ""));
}

/// The archive Extract all applies to: the one being shown, or the one selected.
export function extractTarget(): string | null {
  const a = splitArchive(app.path);
  if (a) return a.file;
  if (selCount() !== 1 || isVirtual(app.path)) return null;
  const i = firstSel();
  const e = i === undefined ? undefined : rows[i];
  return e && !e.is_dir && isArchiveName(e.name) ? entryPath(e) : null;
}

export function extractAll(file = extractTarget()) {
  if (!file) return;
  Object.assign(extractDlg, { open: true, title: "Extract all", file, base: "", picked: [""], dest: defaultDest(file) });
}

/// Extract what's selected inside the archive being shown.
export async function extractSelected() {
  const a = splitArchive(app.path);
  if (!a || !selCount()) return;
  const items = await withSel((s) => invoke<{ path: string }[]>("sel_items", { sel: s })).catch(() => []);
  const picked = items.map((it) => splitArchive(it.path)?.inner ?? "").filter(Boolean);
  if (!picked.length) return;
  const title = picked.length === 1 ? `Extract ${baseName(picked[0])}` : `Extract ${plural(picked.length)}`;
  Object.assign(extractDlg, { open: true, title, file: a.file, base: a.inner, picked, dest: defaultDest(a.file) });
}

export async function browseDest() {
  const start = (await invoke<{ exists: boolean }[]>("path_stamps", { paths: [extractDlg.dest] }).catch(() => []))[0]?.exists
    ? extractDlg.dest
    : parentPath(extractDlg.dest) ?? "";
  const picked = await invoke<string | null>("pick_folder", { start, title: "Select a destination" }).catch((e) => {
    app.status = String(e);
    return null;
  });
  if (picked) extractDlg.dest = picked;
}

export async function runExtract() {
  const d = { ...extractDlg, picked: [...extractDlg.picked] };
  extractDlg.open = false;
  const name = baseName(d.file);
  let r: Extracted;
  try {
    r = await invoke<Extracted>("archive_extract", { req: { file: d.file, base: d.base, picked: d.picked, dest: d.dest } });
  } catch (e) {
    note(`Couldn't extract from ${name}. ${e}`, "error");
    return;
  }
  if (r.made.length) recordMade(r.made);
  const problems: OpItem[] = r.skipped.map((s) => ({
    src: s.name,
    dst: "",
    state: "skipped",
    error: s.why,
    replaced: false,
    bin_id: null,
    size: 0,
    modified: 0,
  }));
  if (r.error && !r.made.length) {
    note(`Couldn't extract from ${name}. ${r.error}`, "error", problems);
  } else if (r.cancelled) {
    note(r.made.length ? `Cancelled. Some items were already extracted to ${baseName(r.dest)}.` : "Cancelled. Nothing was extracted.", "info", problems);
  } else {
    const parts = [`Extracted ${name} to ${baseName(r.dest)}`];
    const n = r.skipped.length;
    if (n) parts.push(`${plural(n)} ${n === 1 ? "wasn't" : "weren't"} extracted`);
    if (r.renamed) parts.push(`${plural(r.renamed, "name")} changed to work on Windows`);
    if (r.kept) parts.push(`${plural(r.kept)} kept as ${r.kept === 1 ? "it was" : "they were"}`);
    if (r.error) parts.push(r.error);
    note(parts.join("; ") + ".", r.skipped.length || r.error ? "warn" : r.renamed || r.kept ? "info" : "ok", problems);
  }
  if (!r.made.length) return;
  if (d.show) await newTab(r.dest);
  else if (isWithin(r.dest, app.path)) await refresh();
}

/// Compress the selection into a ZIP beside it, named after the item the
/// command was given on; the new ZIP comes up selected, ready to rename.
export async function compressSel() {
  if (!selCount() || isVirtual(app.path) || isArchive(app.path)) return;
  const i = focusedRow();
  const e = i === undefined ? undefined : rows[i];
  if (!e) return;
  let r: Compressed;
  try {
    r = await withSel((s) => invoke<Compressed>("archive_compress", { sel: s, nameFrom: e.name }));
  } catch (err) {
    note(`Couldn't compress. ${err}`, "error");
    return;
  }
  if (r.error) return note(`Couldn't compress. ${r.error}`, "error");
  if (r.cancelled || !r.made) return note("Cancelled. No ZIP file was made.", "info");
  recordMade([r.made]);
  note(`Compressed to ${baseName(r.made.path)}`, "ok");
  await showResult([r.made.path], true);
}
