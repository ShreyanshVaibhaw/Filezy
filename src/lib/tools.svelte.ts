// Optional tools (O01): checksums, byte comparison and folder sizes, run in
// tools.rs and shown by Tools.svelte. One tool at a time; closing or Stop
// cancels the backend job and keeps what was already worked out.
import { invoke } from "@tauri-apps/api/core";
import { app, withSel } from "./store.svelte";
import type { BatchItem } from "./ops.svelte";

export const ALGOS = ["MD5", "SHA-1", "SHA-256", "SHA-512"];

export interface HashResult {
  path: string;
  size: number;
  hashes: [string, string][];
  error: string | null;
}
export interface Compared {
  same: boolean;
  size_a: number;
  size_b: number;
  first_diff: number | null;
}
export interface Size {
  job: number;
  path: string;
  is_dir: boolean;
  bytes: number;
  files: number;
  folders: number;
  skipped: number;
  links: number;
  done: boolean;
}

export const tool = $state({
  kind: "" as "" | "hash" | "compare" | "sizes",
  title: "",
  job: 0,
  busy: false,
  done: 0,
  total: 0,
  paths: [] as string[],
  algos: ["SHA-256"] as string[],
  hashes: [] as HashResult[],
  compared: null as Compared | null,
  sizes: [] as Size[],
  /** Measuring what's in one folder (the storage view), not a selection. */
  dir: "",
  error: "",
});

let nextJob = 1;
function begin(kind: typeof tool.kind, title: string) {
  stopTool();
  Object.assign(tool, { kind, title, job: nextJob++, busy: true, done: 0, total: 0, hashes: [], compared: null, sizes: [], dir: "", error: "" });
  return tool.job;
}

export function stopTool() {
  if (tool.busy) invoke("cancel_job", { job: tool.job }).catch(() => {});
  tool.busy = false;
}
export function closeTool() {
  stopTool();
  tool.kind = "";
}

export function onProgress(p: { job: number; done: number; total: number }) {
  if (p.job !== tool.job) return;
  tool.done = p.done;
  tool.total = p.total;
}

export function onSize(s: Size) {
  if (s.job !== tool.job) return;
  const i = tool.sizes.findIndex((x) => x.path === s.path);
  if (i >= 0) tool.sizes[i] = s;
  else tool.sizes.push(s);
}

const selItems = () => withSel((sel) => invoke<BatchItem[]>("sel_items", { sel })).catch((e) => ((app.status = String(e)), [] as BatchItem[]));

export async function checksumsSel() {
  const files = (await selItems()).filter((it) => !it.is_dir).map((it) => it.path);
  if (!files.length) return void (app.status = "Checksums are for files. Select one or more files.");
  tool.paths = files;
  await runHashes();
}

/** Hash tool.paths with the chosen algorithms; again whenever the choice changes. */
export async function runHashes() {
  const job = begin("hash", "Checksums");
  try {
    const r = await invoke<HashResult[]>("hash_files", { job, paths: tool.paths, algos: tool.algos });
    if (job === tool.job) tool.hashes = r;
  } catch (e) {
    if (job === tool.job) tool.error = String(e);
  } finally {
    if (job === tool.job) tool.busy = false;
  }
}

export async function compareSel() {
  const files = (await selItems()).filter((it) => !it.is_dir).map((it) => it.path);
  if (files.length !== 2) return void (app.status = "Select exactly two files to compare.");
  const job = begin("compare", "Compare files");
  tool.paths = files;
  try {
    const r = await invoke<Compared>("compare_files", { job, a: files[0], b: files[1] });
    if (job === tool.job) tool.compared = r;
  } catch (e) {
    if (job === tool.job) tool.error = String(e);
  } finally {
    if (job === tool.job) tool.busy = false;
  }
}

async function measure(dir: string | null, paths: string[]) {
  const job = begin("sizes", dir ? "Folder sizes" : "Size");
  tool.dir = dir ?? "";
  tool.paths = paths;
  try {
    await invoke("measure_sizes", { job, dir, paths });
  } catch (e) {
    if (job === tool.job) tool.error = String(e);
  } finally {
    if (job === tool.job) tool.busy = false;
  }
}

export async function sizesSel() {
  const items = await selItems();
  if (items.length) await measure(null, items.map((it) => it.path));
}

/** The storage view: everything directly in the current folder, largest first. */
export function analyzeFolder() {
  if (app.path) void measure(app.path, []);
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v < 10 ? v.toFixed(2) : v < 100 ? v.toFixed(1) : Math.round(v)} ${units[u]}`;
}
