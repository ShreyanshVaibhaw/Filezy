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
  /** Measuring what's in one folder (the storage view), not a selection. */
  dir: "",
  error: "",
});

// Folder sizes arrive as one event per item. They collect in a plain map and
// reach the dialog at most once a frame, sorted largest first, so thousands of
// items cost one render each frame rather than one each.
const sizeMap = new Map<string, Size>();
let sizeList = $state.raw<Size[]>([]);
let sizeFrame = 0;
export const sizes = {
  get list() {
    return sizeList;
  },
};
function publishSizes() {
  cancelAnimationFrame(sizeFrame);
  sizeFrame = 0;
  sizeList = [...sizeMap.values()].sort((a, b) => b.bytes - a.bytes);
}

let nextJob = 1;
function begin(kind: typeof tool.kind, title: string) {
  stopTool();
  Object.assign(tool, { kind, title, job: nextJob++, busy: true, done: 0, total: 0, hashes: [], compared: null, dir: "", error: "" });
  sizeMap.clear();
  publishSizes();
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
  sizeMap.set(s.path, s);
  sizeFrame ||= requestAnimationFrame(publishSizes);
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
    if (job === tool.job) {
      tool.busy = false;
      publishSizes();
    }
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
