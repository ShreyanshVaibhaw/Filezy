/// In-app updates. The app asks GitHub for the latest release manifest, verifies
/// the download against the bundled public key, and hands the installer to
/// Windows. Only an installed copy can be replaced, so a portable run is told so
/// instead of being offered an update it cannot apply.
import { invoke } from "@tauri-apps/api/core";
import type { Update } from "@tauri-apps/plugin-updater";

type Phase = "idle" | "checking" | "none" | "found" | "downloading" | "ready" | "error";

export const updater = $state({
  phase: "idle" as Phase,
  version: "",
  notes: "",
  percent: 0,
  message: "",
  installed: true, // assume yes until the backend says otherwise
});

invoke<boolean>("is_installed")
  .then((v) => (updater.installed = v))
  .catch(() => {});

/// The updater plugin's errors in plain words; anything else keeps its own text.
export function explainUpdateError(e: unknown): string {
  const raw = String(e).replace(/`/g, "").trim();
  if (/release JSON|error sending request|dns|connect/i.test(raw)) return "Couldn't get update information from the server. Check your connection and try again later.";
  const status = /status: (\d{3})/i.exec(raw);
  if (status) return `The update couldn't be downloaded (the server answered ${status[1]}). Try again later.`;
  if (/signature/i.test(raw)) return "The downloaded update failed its signature check, so it wasn't installed.";
  return raw;
}

export async function checkForUpdates() {
  if (updater.phase === "checking" || updater.phase === "downloading") return;
  updater.phase = "checking";
  updater.message = "";
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) {
      updater.phase = "none";
      return;
    }
    updater.version = update.version;
    updater.notes = update.body ?? "";
    updater.phase = "found";
    pending = update;
  } catch (e) {
    updater.phase = "error";
    updater.message = explainUpdateError(e);
  }
}

// The Update handle owns the download; keep it between check and install.
let pending: Update | null = null;

export async function downloadAndInstall() {
  if (!pending) return;
  updater.phase = "downloading";
  updater.percent = 0;
  let total = 0;
  let got = 0;
  try {
    await pending.downloadAndInstall((p) => {
      if (p.event === "Started") total = p.data?.contentLength ?? 0;
      else if (p.event === "Progress") {
        got += p.data?.chunkLength ?? 0;
        updater.percent = total ? Math.round((got / total) * 100) : 0;
      } else if (p.event === "Finished") updater.percent = 100;
    });
    updater.phase = "ready";
  } catch (e) {
    updater.phase = "error";
    updater.message = explainUpdateError(e);
  }
}

export async function restartApp() {
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
