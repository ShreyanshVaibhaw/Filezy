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
    updater.message = String(e);
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
    updater.message = String(e);
  }
}

export async function restartApp() {
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
