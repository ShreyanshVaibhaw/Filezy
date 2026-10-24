// Process startup timing (Q02): from spawning filezy.exe with a folder to the
// page's first row on screen, against a throwaway profile. Works on a shipped
// build: it only reads the page over DevTools and sends no input.
//
//   node scripts/e2e/startup.mjs [--exe=<path>] [--runs=5]
//
// The first run creates the WebView2 profile, so it is reported apart. The
// folder's 1,000 files are warm in the OS cache after that run; a cold-cache
// start needs a reboot or an elevated standby-list flush and isn't measured.
import { spawn, execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { connect, freePort, profileEnv } from "./cdp.mjs";
import { cargoTarget } from "../cargo-target.mjs";

const arg = (k, d) => process.argv.find((a) => a.startsWith(`--${k}=`))?.slice(k.length + 3) ?? d;
const exe = path.resolve(arg("exe") ?? path.join(cargoTarget(), "release", "filezy.exe"));
const runs = Number(arg("runs", "5"));
const root = fs.mkdtempSync(path.join(os.tmpdir(), "filezy-startup-"));
const dir = path.join(root, "folder");
fs.mkdirSync(dir);
for (let i = 0; i < 1000; i++) fs.writeFileSync(path.join(dir, `file-${String(i).padStart(4, "0")}.txt`), "");

async function once() {
  const port = await freePort();
  const t0 = Date.now();
  const proc = spawn(exe, [dir], { env: profileEnv(root, port), stdio: "ignore" });
  try {
    const cdp = await connect(port);
    // Both times come from the page's clock, so DevTools' own start-up delay doesn't count.
    // DevTools can attach to the blank page before the app's page replaces it: retry then.
    const evRetry = async (js) => {
      for (let i = 0; ; i++) {
        try {
          return await cdp.ev(js);
        } catch (e) {
          if (i > 300 || !/context was destroyed|Cannot find context/.test(String(e))) throw e;
          await new Promise((r) => setTimeout(r, 20));
        }
      }
    };
    const r = await evRetry(`(async () => {
      while (!document.querySelector("[data-idx]")) await new Promise((r) => setTimeout(r, 2));
      const nav = performance.getEntriesByType("navigation")[0];
      return { origin: performance.timeOrigin, loaded: nav ? nav.loadEventEnd : 0, rows: performance.now() };
    })()`);
    cdp.close();
    return { page: Math.round(r.origin - t0), loaded: Math.round(r.origin + r.loaded - t0), rows: Math.round(r.origin + r.rows - t0) };
  } finally {
    const exited = new Promise((r) => proc.once("exit", r));
    execFile("taskkill.exe", ["/T", "/F", "/PID", String(proc.pid)], () => {});
    await exited;
  }
}

const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
try {
  const first = await once();
  console.log(`first start (new WebView2 profile): page created ${first.page} ms, loaded ${first.loaded} ms, first rows ${first.rows} ms`);
  const rest = [];
  for (let i = 0; i < runs; i++) rest.push(await once());
  for (const k of ["page", "loaded", "rows"]) {
    const xs = rest.map((r) => r[k]);
    console.log(`later starts, ${k}: ${xs.join(", ")} ms (median ${median(xs)} ms)`);
  }
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
