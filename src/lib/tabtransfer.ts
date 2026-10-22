// Windows share a WebView2 profile. A per-transfer record and acknowledgement
// let the source keep its tab until the receiving window has opened it.
import { invoke } from "@tauri-apps/api/core";
import { app, tabs, exportTab, importTab, closeTab } from "./store.svelte";

const PREFIX = "filezy.transfer.";
const WINDOWS = "filezy.window.";
const TTL = 30_000;
let identity = "";
let incoming: Promise<unknown> = Promise.resolve();
const sending = new Set<number>();

interface Transfer { to: string; from: string; stamp: number; tab: unknown; ack?: boolean }
function read(key: string): Transfer | null {
  try { return JSON.parse(localStorage.getItem(key) ?? "null"); } catch { return null; }
}

function receive(key: string, token?: string): Promise<boolean> {
  const next = incoming.then(() => receiveOne(key, token));
  incoming = next.catch(() => {});
  return next;
}

async function receiveOne(key: string, token?: string): Promise<boolean> {
  const record = read(key);
  if (!record || record.ack || record.from === identity || record.to !== (token ?? identity) ||
      !Number.isFinite(record.stamp) || Date.now() - record.stamp > TTL) return false;
  try {
    await importTab(record.tab);
    localStorage.setItem(key, JSON.stringify({ ...record, ack: true }));
    return true;
  } catch (error) { app.status = `The tab could not be moved. ${error}`; return false; }
}

export async function initTabTransfers(): Promise<{ imported: boolean; destroy: () => void }> {
  identity = await invoke<string>("window_identity");
  const advertise = () => localStorage.setItem(WINDOWS + identity, JSON.stringify({ stamp: Date.now(), name: tabs.map((t) => t.path).join(", ") || "Filezy" }));
  advertise();
  const heartbeat = setInterval(advertise, 5000);
  const listener = (event: StorageEvent) => { if (event.key?.startsWith(PREFIX)) void receive(event.key); };
  window.addEventListener("storage", listener);
  const cleanup = () => localStorage.removeItem(WINDOWS + identity);
  window.addEventListener("pagehide", cleanup);
  // Expire abandoned handoffs without touching a window's live records.
  for (const key of Object.keys(localStorage)) {
    if ((key.startsWith(PREFIX) || key.startsWith(WINDOWS)) && Date.now() - (read(key)?.stamp ?? 0) > TTL) localStorage.removeItem(key);
  }
  const token = await invoke<string | null>("tab_launch_token");
  return { imported: token ? await receive(PREFIX + token, token) : false, destroy() {
    clearInterval(heartbeat); cleanup(); window.removeEventListener("storage", listener); window.removeEventListener("pagehide", cleanup);
  } };
}

export function otherTabWindows(): { id: string; name: string }[] {
  const out = [];
  for (const key of Object.keys(localStorage)) {
    if (!key.startsWith(WINDOWS) || key === WINDOWS + identity) continue;
    try {
      const record = JSON.parse(localStorage.getItem(key)!);
      if (Date.now() - record.stamp < 15_000) out.push({ id: key.slice(WINDOWS.length), name: String(record.name) });
    } catch { /* A closing window may have removed the record. */ }
  }
  return out;
}

export async function transferTab(id: number, target?: string): Promise<boolean> {
  if (sending.has(id)) return false;
  sending.add(id);
  let key = "";
  try {
    const tab = await exportTab(id);
    if (!tab) return false;
    const token = crypto.randomUUID();
    key = PREFIX + token;
    localStorage.setItem(key, JSON.stringify({ to: target ?? token, from: identity, stamp: Date.now(), tab }));
    if (!target) await invoke("new_window", { transfer: token });
    const until = Date.now() + 20_000;
    while (Date.now() < until) {
      if (read(key)?.ack) { closeTab(id); return true; }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw new Error("The other window did not accept the tab. It is still open here.");
  } catch (error) { app.status = String(error); return false; }
  finally { if (key) localStorage.removeItem(key); sending.delete(id); }
}

export async function dropTab(id: number, x: number, y: number) {
  const target = await invoke<string>("window_at", { x, y, scale: devicePixelRatio });
  // Leaving the strip vertically also detaches when the release is still over
  // this window's content. Releases in the strip never call this function.
  if (target === identity) { await transferTab(id); return; }
  const window = otherTabWindows().find((w) => w.id === target);
  await transferTab(id, window?.id);
}
