// Native E2E runner. Launches the built Filezy exe against a throwaway profile,
// drives the real WebView2 UI over the DevTools protocol, and checks the results
// on disk. Nothing here touches the user's own settings, pins, or files:
// WebView2 storage, %APPDATA%, the app's %TEMP% and every fixture live under
// one temp folder.
//
//   npm run test:e2e                     every suite against <target>-e2e/release/filezy.exe
//   npm run test:e2e -- --debug          use <target>-e2e/debug/filezy.exe
//   npm run test:e2e -- nav undo         only tests whose name contains a filter
//   npm run test:e2e -- --keep           keep the temp folder for inspection
//   npm run test:e2e -- --perf           also run the slow performance measurements
//   npm run test:e2e -- --no-input       skip tests that take the foreground or
//                                        replace the clipboard (safe while someone works)
//
// Build first: `npm run build:e2e` (or `npm run build:e2e -- --debug`), which
// writes next to Cargo's build folder (src-tauri/target-e2e by default). The hooks the runner drives exist only in that
// build (the `e2e` feature); a shipped build ignores FILEZY_E2E. A plain
// `cargo build` exe loads the Vite dev server instead of the bundled UI.
//
// Every test starts from one tab, nothing selected, empty undo history, no
// message or dialog showing, and an idle listing. A failed test gets a fresh
// app process, so whatever it left open can't break the next one. A test that
// passes but left uncaught page exceptions or console errors fails.
//
// Side effects outside the temp folder, all deliberate: keyboard tests bring the
// app window to the front, clipboard tests replace the clipboard contents, and
// Recycle Bin tests recycle and restore files whose names carry a per-run token
// (leftovers are restored when the run ends). Drag tests never touch the real
// mouse: the app's own drop target is driven from a script (see e2edrag.rs).
// The indexed-search test needs a folder the Windows index covers, which the
// temp folder never is: it writes three small files under
// %USERPROFILE%\filezy-e2e-index-<token> and removes that folder when it ends.
// The devices tests make drive letters with `subst` (this session only) for
// folders in the temp folder, and remove them when each test ends.

import { spawn, execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { connect, freePort, profileEnv, sleep } from "./cdp.mjs";
import { E2E_TARGET } from "../build-e2e.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const has = (f) => args.includes(f);
const noInput = has("--no-input");
const filters = args.filter((a) => !a.startsWith("--"));
const exeArg = args.find((a) => a.startsWith("--exe="))?.slice(6);
const exe = path.resolve(exeArg ?? path.join(E2E_TARGET, has("--debug") ? "debug" : "release", "filezy.exe"));
if (!fs.existsSync(exe)) {
  console.error(`missing ${exe}\nbuild it with: npm run build:e2e${has("--debug") ? " -- --debug" : ""}`);
  process.exit(2);
}

const root = fs.mkdtempSync(path.join(os.tmpdir(), "filezy-e2e-"));
// Unique per run. Recycle Bin tests put it in every file name so they can only
// ever match their own items, never something the user deleted.
const token = path.basename(root).replace(/[^a-z0-9]/gi, "").slice(-6).toLowerCase();
const fixRoot = path.join(root, "fix");
fs.mkdirSync(fixRoot);

let proc = null;
let cdp = null;

/** Thrown by t.skip: the machine can't run this test (reported, not failed). */
class Skip extends Error {}

let port = 0;
async function launch(extraArgs = []) {
  port = await freePort();
  proc = spawn(exe, extraArgs, {
    env: {
      ...profileEnv(root, port),
      FILEZY_E2E: "1",
      // A small result cap, so the capped-search test needs 2,000 files, not 100,000.
      FILEZY_E2E_SEARCH_MAX: "2000",
      // The same for shell menus, so the over-the-limit note needs 201 files, not 10,001.
      FILEZY_E2E_MENU_MAX: "200",
      // A pause per 256 KB extracted or compressed, so a 48 MB archive is slow
      // enough to show the progress dialog and be cancelled.
      FILEZY_E2E_ARCHIVE_DELAY_MS: "25",
    },
    stdio: "ignore",
  });
  cdp = await connect(port);
  cdp.on("Runtime.exceptionThrown", (p) => pageErrors.push(p.exceptionDetails.exception?.description ?? p.exceptionDetails.text));
  cdp.on("Runtime.consoleAPICalled", (p) => {
    if (p.type === "error") pageErrors.push(p.args.map((a) => a.value ?? a.description).join(" "));
  });
  await cdp.send("Runtime.enable");
  // Key events reach the page only while it has focus; keep it focused even
  // when another window is in front, so keyboard tests don't depend on that.
  await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true });
  await waitFor(
    "!!globalThis.__e2e && __e2e.app.path !== ''",
    30000,
    "app never became ready (a build without the e2e feature has no test hooks: npm run build:e2e)",
  );
}

// Uncaught page exceptions and console.error output since the current test began.
let pageErrors = [];
// Whether the app window was brought to the foreground during the current test.
let foregrounded = false;

// Put the app back to how every test expects to find it. A question still
// open is answered No first, and the operation waiting on it allowed to
// finish, so it can't add to the history after the history is cleared.
async function reset() {
  await cdp.ev("__e2e.dialog.ask?.resolve(false)");
  await waitFor("__e2e.idle()", 15000, "the app never went idle after the last test");
  await cdp.ev(`(() => {
    const e = __e2e;
    e.extractDlg.open = false;
    e.batch.open = false;
    e.closeTool();
    e.activity.open = false;
    e.activity.current = null;
    // Keep the shown tab: switching to another would re-run that tab's search.
    for (const x of e.tabs.filter((x) => x.id !== e.app.activeTab)) e.closeTab(x.id);
    e.stopSearch();
    e.app.renaming = -1;
    e.clearSel();
    e.history.undo.length = 0;
    e.history.redo.length = 0;
    e.app.status = "";
    e.app.statusInfo = "";
    // Focus left on a command button or menu would keep file-list keys from the list.
    document.activeElement?.blur?.();
  })()`);
  await waitFor("__e2e.idle()", 15000, "the app never went idle after the reset");
}

// Counts drags the page has seen end (dropped or left), from the same "drag"
// event the app's own drop handling listens to. Registered after the app's
// listener, so by the time the count moves the app has started its drop.
// Installed on first use, and again after a restart or a page reload.
const DRAG_TRACKER = `globalThis.__e2eDrags ?? ((s) => __e2e.invoke("plugin:event|listen", {
  event: "drag",
  target: { kind: "Any" },
  handler: __TAURI_INTERNALS__.transformCallback(({ payload }) => {
    if (payload.kind === "enter") s.open = true;
    else if (payload.kind !== "over" && s.open) {
      s.open = false;
      s.ended++;
    }
  }),
}).then(() => (globalThis.__e2eDrags = s)))({ open: false, ended: 0 })`;

/** Run a scripted drag and wait until the page has handled how it ended. */
async function tracked(drag) {
  const before = (await cdp.ev(DRAG_TRACKER)).ended;
  const r = await drag();
  await waitFor(`__e2eDrags.ended > ${before}`, 5000, "the page never saw the drag end");
  return r;
}

async function quit() {
  cdp?.close();
  cdp = null;
  if (proc && proc.exitCode === null) {
    const exited = new Promise((r) => proc.once("exit", r));
    // /T also ends any second window the app spawned (Ctrl+N).
    execFile("taskkill.exe", ["/T", "/F", "/PID", String(proc.pid)], () => {});
    await exited;
  }
  proc = null;
  // WebView2 hands a new app its profile's running browser process, whose
  // DevTools port is the old one: wait until every process of the profile is gone.
  const profile = path.join(root, "webview").replace(/'/g, "''");
  await t.psCmd(`
    $deadline = (Get-Date).AddSeconds(15)
    do {
      $left = @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" | Where-Object { $_.CommandLine -and $_.CommandLine.Contains('${profile}') })
      $left | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
      if ($left.Count) { Start-Sleep -Milliseconds 100 }
    } while ($left.Count -and (Get-Date) -lt $deadline)`);
}

async function waitFor(expr, timeout = 5000, msg) {
  const deadline = Date.now() + timeout;
  let last;
  for (;;) {
    try {
      last = await cdp.ev(expr);
      if (last) return last;
    } catch (e) {
      last = e.message;
    }
    if (Date.now() > deadline) throw new Error(`${msg ?? "timed out"}: ${expr} -> ${JSON.stringify(last)}`);
    await sleep(50);
  }
}

// Windows virtual-key codes for the non-character keys tests press.
const KEYS = {
  Enter: 13, Escape: 27, Tab: 9, Backspace: 8, Delete: 46, " ": 32,
  ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40,
  Home: 36, End: 35, PageUp: 33, PageDown: 34,
  F2: 113, F3: 114, F5: 116, F6: 117, F10: 121, ContextMenu: 93,
};

const t = {
  root,
  token,
  fixRoot,
  ev: (expr) => cdp.ev(expr),
  send: (method, params) => cdp.send(method, params),
  waitFor,
  sleep,

  /** Call a function on the page's __e2e hook with JSON arguments. */
  call(fn, ...a) {
    return cdp.ev(`__e2e.${fn}(${a.map((x) => JSON.stringify(x)).join(",")})`);
  },

  /** Navigate the active tab and wait for the listing to settle. */
  async nav(p) {
    const ok = await t.call("navigate", p);
    if (!ok) throw new Error(`navigate failed: ${p} (${await cdp.ev("__e2e.app.status")})`);
    await t.settle();
  },

  /** Wait until the active listing has no outstanding row fetches. */
  async settle() {
    await waitFor("__e2e.idle()", 10000, "listing never settled");
  },

  /** Names in the current listing, in display order. */
  names: () => cdp.ev("__e2e.listing().then((r) => r.map((e) => e.name))"),

  /** Select items by name (replaces the selection). */
  select: (names) => cdp.ev(`__e2e.selectNames(${JSON.stringify(names)})`),

  selectedNames: () => cdp.ev("__e2e.selectedNames()"),

  /**
   * WebView2 only takes keyboard input while its window is the foreground
   * window, so the first key press of each test brings this app's process
   * forward (by process id, never by title, so another Filezy can't be hit).
   */
  async foreground() {
    if (noInput) t.skip("takes the foreground (run without --no-input)");
    if (foregrounded) return;
    await t.psCmd(`(New-Object -ComObject WScript.Shell).AppActivate(${proc.pid}) | Out-Null`);
    await sleep(150);
    foregrounded = true;
  },

  async key(key, { ctrl = false, shift = false, alt = false } = {}) {
    await t.foreground();
    const modifiers = (alt ? 1 : 0) | (ctrl ? 2 : 0) | (shift ? 8 : 0);
    const named = KEYS[key];
    const ch = key.length === 1 ? key : "";
    const code = named ?? ch.toUpperCase().charCodeAt(0);
    // Enter carries a "\r" character, which is what activates a focused button.
    const text = key === "Enter" && !ctrl && !alt ? "\r" : ch && !ctrl && !alt ? ch : undefined;
    const base = { modifiers, key, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code };
    await cdp.send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", text, ...base });
    await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  },

  async type(text) {
    for (const ch of text) await t.key(ch);
  },

  /**
   * Center point of the first element matching a CSS selector, in CSS pixels.
   * `where` narrows the match by exact title attribute, trimmed text, or
   * contained text (`has`, for buttons that start with an icon glyph).
   */
  async center(selector, where = {}) {
    const r = await cdp.ev(`(() => {
      const w = ${JSON.stringify(where)};
      const el = [...document.querySelectorAll(${JSON.stringify(selector)})].find((el) =>
        (w.title === undefined || el.title === w.title) &&
        (w.text === undefined || el.textContent.trim() === w.text) &&
        (w.has === undefined || el.textContent.includes(w.has)));
      if (!el) return null;
      const b = el.getBoundingClientRect();
      return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
    })()`);
    if (!r) throw new Error(`no element for ${selector} ${JSON.stringify(where)}`);
    return r;
  },

  async click(target, { button = "left", count = 1, ctrl = false, shift = false, where } = {}) {
    const p = typeof target === "string" ? await t.center(target, where) : target;
    const modifiers = (ctrl ? 2 : 0) | (shift ? 8 : 0);
    const base = { x: p.x, y: p.y, button, modifiers };
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: p.x, y: p.y, modifiers });
    for (let i = 1; i <= count; i++) {
      await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", clickCount: i, ...base });
      await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", clickCount: i, ...base });
    }
  },

  async hover(target) {
    const p = typeof target === "string" ? await t.center(target) : target;
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: p.x, y: p.y });
  },

  async screenshot(name) {
    // The window is transparent (mica), so paint the theme's fallback color
    // behind the page for the capture.
    await cdp.ev(`document.documentElement.style.background = "var(--mica-fallback)"`);
    const { data } = await cdp.send("Page.captureScreenshot", { format: "png" });
    await cdp.ev(`document.documentElement.style.background = ""`);
    const file = path.join(root, "shots", `${name}.png`);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, Buffer.from(data, "base64"));
    return file;
  },

  /** A fresh fixture folder for one test. */
  dir(rel) {
    const p = path.join(fixRoot, rel);
    fs.mkdirSync(p, { recursive: true });
    return p;
  },

  /** Write a fixture file; `mtime` is a Date or epoch ms. */
  mk(p, content = "", mtime) {
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, content);
    if (mtime !== undefined) {
      const d = new Date(mtime);
      fs.utimesSync(p, d, d);
    }
    return p;
  },

  exists: (p) => fs.existsSync(p),
  /** Read a file, waiting out a moment where the copy engine still has it open. */
  read(p) {
    for (let i = 0; ; i++) {
      try {
        return fs.readFileSync(p, "utf8");
      } catch (e) {
        if (i > 40 || !["EBUSY", "EPERM"].includes(e.code)) throw e;
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50);
      }
    }
  },

  /** Run a PowerShell helper from scripts/e2e (native dialogs, attributes, ACLs). */
  ps(script, ...psArgs) {
    return new Promise((resolve, reject) => {
      execFile(
        "powershell.exe",
        ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(here, script), ...psArgs],
        { windowsHide: true, timeout: 60000 },
        (err, stdout, stderr) => (err ? reject(new Error(stderr || err.message)) : resolve(stdout.trim())),
      );
    });
  },

  /**
   * A drag in from another app, scripted through the app's own drop target
   * (see e2edrag.rs). `points` are CSS pixels in the window, each with an
   * optional rest in ms and the modifiers held from there on (keys:
   * "ctrl+shift", or "none"); the drop happens at the last one. Returns what
   * the drag's source would see: hr, effect, performed, logical, feedback.
   * Returns once the page has started acting on the drop, so t.settle() then
   * waits for the operation it started.
   */
  drag: (files, points, { keys = "", right = false, cancel = false } = {}) =>
    tracked(() => cdp.ev(`__e2e.invoke("e2e_drag", ${JSON.stringify({ paths: files, script: { points, keys, right, cancel } })})`)),

  /**
   * A drag that starts in the app: the button goes down at the first point and
   * the page starts its drag on the way to the second, as for a user; the rest
   * of the path is scripted as for drag(). `points` as for drag().
   */
  mouseDrag: (points, { ctrl = false, right = false } = {}) =>
    tracked(async () => {
      const [from, ...rest] = points;
      const button = right ? "right" : "left";
      const modifiers = ctrl ? 2 : 0;
      const mouse = (type, p, extra = {}) => cdp.send("Input.dispatchMouseEvent", { type, x: p.x, y: p.y, modifiers, ...extra });
      await cdp.ev(`__e2e.invoke("e2e_arm_drag", ${JSON.stringify({ script: { points: rest, keys: ctrl ? "ctrl" : "", right } })})`);
      await mouse("mouseMoved", from);
      await mouse("mousePressed", from, { button, clickCount: 1 });
      await mouse("mouseMoved", rest[0], { button, buttons: right ? 2 : 1 });
      try {
        return await cdp.ev(`__e2e.invoke("e2e_drag_outcome")`);
      } finally {
        await mouse("mouseReleased", rest.at(-1), { button, clickCount: 1 });
      }
    }),

  /** Press, move in `steps` and release the left button inside the page (no OS drag). */
  async pageDrag(from, to, steps = 10) {
    const m = (type, p, extra) => cdp.send("Input.dispatchMouseEvent", { type, x: p.x, y: p.y, ...extra });
    await m("mouseMoved", from, { button: "none" });
    await m("mousePressed", from, { button: "left", buttons: 1, clickCount: 1 });
    for (let i = 1; i <= steps; i++) {
      const p = { x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps };
      await m("mouseMoved", p, { button: "left", buttons: 1 });
    }
    await m("mouseReleased", to, { button: "left", buttons: 0, clickCount: 1 });
  },

  /** Answer the app's own confirmation dialog (Yes/OK or No/Cancel); returns its message. */
  answer: (ok) =>
    cdp.ev(`(async () => {
      for (let i = 0; i < 100 && !__e2e.dialog.ask; i++) await new Promise((r) => setTimeout(r, 50));
      if (!__e2e.dialog.ask) throw new Error("no dialog");
      const text = __e2e.dialog.ask.message;
      __e2e.dialog.ask.resolve(${!!ok});
      return text;
    })()`),

  /** Click a button in a native dialog owned by the app (see uia.ps1). */
  uia: (...a) => t.ps("uia.ps1", "-ProcId", String(proc.pid), ...a),

  /** Process id of the app under test. */
  get pid() {
    return proc.pid;
  },

  /** Run one PowerShell command line and return its trimmed output. */
  psCmd(command) {
    return new Promise((resolve, reject) => {
      execFile(
        "powershell.exe",
        ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command],
        { windowsHide: true, timeout: 60000 },
        (err, stdout, stderr) => (err ? reject(new Error(stderr || err.message)) : resolve(stdout.trim())),
      );
    });
  },

  /** The clipboard's text (Copy as path). */
  clipText: () => t.psCmd("Get-Clipboard -Raw"),

  /**
   * Copy as path through the app, as the context menu does, and read it back.
   * Retried because other programs on a test machine can replace the clipboard.
   */
  async copyAsPath() {
    if (noInput) t.skip("replaces the clipboard (run without --no-input)");
    let text = "";
    for (let i = 0; i < 4 && !text; i++) {
      await cdp.ev(`__e2e.withSel((sel) => __e2e.invoke("copy_as_path", { sel }))`);
      text = await t.clipText();
    }
    return text;
  },

  /** The system clipboard's files and drop effect (2 = cut, 5 = copy). */
  async clipboard() {
    const r = JSON.parse(await t.ps("clip.ps1"));
    return { files: [].concat(r.files ?? []), effect: r.effect };
  },

  /**
   * Copy or cut the selection through the app and make sure it really is on
   * the clipboard: other programs on a test machine can replace it any time.
   */
  async copyToClipboard(cut = false) {
    if (noInput) t.skip("replaces the clipboard (run without --no-input)");
    const want = (await t.selectedNames()).length;
    for (let i = 0; i < 4; i++) {
      await t.call("copySel", cut);
      const c = await t.clipboard();
      if (c.files.length === want && c.effect === (cut ? 2 : 5)) return;
      await sleep(200);
    }
    throw new Error("another program kept replacing the clipboard");
  },

  /** Set the clipboard from another process (clip.ps1 `args`) until the page sees `cond`. */
  async setClipboard(args, cond) {
    if (noInput) t.skip("replaces the clipboard (run without --no-input)");
    for (let i = 0; ; i++) {
      await t.ps("clip.ps1", ...args);
      try {
        return await waitFor(cond, 3000, `clipboard ${args.join(" ")}`);
      } catch (e) {
        if (i >= 3) throw e;
      }
    }
  },

  /** "C:\x" -> "\\localhost\C$\x": the same file through the loopback share, a different volume. */
  unc: (p) => `\\\\localhost\\${p[0].toUpperCase()}$${p.slice(2)}`,

  /** Names inside a ZIP as .NET reads them, sorted; waits while Windows is still writing it. */
  async zipEntries(zip) {
    const read = `Add-Type -AssemblyName System.IO.Compression.FileSystem; $z = [IO.Compression.ZipFile]::OpenRead('${zip}'); ConvertTo-Json -Compress @($z.Entries | ForEach-Object FullName); $z.Dispose()`;
    for (let i = 0; ; i++) {
      try {
        return JSON.parse(await t.psCmd(read)).sort();
      } catch (e) {
        if (i >= 60) throw e;
        await sleep(500);
      }
    }
  },

  /** The point to click on a Recycle Bin page row, once listed: by contained text (`has`) or exact `title`. */
  async binRow({ has, title }) {
    const find = `[...document.querySelectorAll(".rrow")].find((r) => ${has !== undefined ? `r.textContent.includes(${JSON.stringify(has)})` : `r.title === ${JSON.stringify(title)}`})`;
    await waitFor(`!!${find}`, 10000, "the Recycle Bin page never listed the item");
    return cdp.ev(`(() => { const b = ${find}.getBoundingClientRect(); return { x: b.left + 40, y: b.top + b.height / 2 }; })()`);
  },

  /** Poll a Node-side condition (disk state) until it holds. */
  async until(cond, timeout = 10000, msg = "condition never held") {
    const deadline = Date.now() + timeout;
    while (!(await cond())) {
      if (Date.now() > deadline) throw new Error(msg);
      await sleep(50);
    }
  },

  async restart(extraArgs) {
    // Close the window the way a user does, so WebView2 writes out what the
    // page stored (a forced kill loses it); force only if that hangs.
    if (proc && proc.exitCode === null && cdp) {
      const exited = new Promise((r) => proc.once("exit", r));
      await cdp.ev("void __e2e.closeWindow?.()").catch(() => {});
      await Promise.race([exited, sleep(10000)]);
    }
    await quit();
    await launch(extraArgs);
  },

  /** A DevTools session on another window of this run (e.g. one Ctrl+N opened), once its page is ready. */
  async otherWindow(timeout = 20000) {
    const mine = cdp.id;
    const deadline = Date.now() + timeout;
    const w = await connect(port, timeout, (pages) => pages.find((p) => p.id !== mine));
    while (!(await w.ev("!!globalThis.__e2e && __e2e.app.path !== ''").catch(() => false))) {
      if (Date.now() > deadline) {
        w.close();
        throw new Error("the other window's page never became ready");
      }
      await sleep(50);
    }
    return w;
  },

  /** End the test as skipped, saying why (a Windows feature this machine lacks). */
  skip(why) {
    throw new Skip(why);
  },

  ok(cond, msg = "expected truthy") {
    if (!cond) throw new Error(msg);
  },
  eq(actual, expected, msg = "") {
    const a = JSON.stringify(actual);
    const e = JSON.stringify(expected);
    if (a !== e) throw new Error(`${msg ? msg + ": " : ""}expected ${e}, got ${a}`);
  },
};

const suiteDir = path.join(here, "suites");
const tests = [];
for (const f of fs.readdirSync(suiteDir).filter((f) => f.endsWith(".mjs")).sort()) {
  const mod = await import(pathToFileURL(path.join(suiteDir, f)).href);
  for (const test of mod.default) {
    const name = `${f.replace(/\.mjs$/, "")}: ${test.name}`;
    if (test.perf && !has("--perf")) continue;
    if (!filters.length || filters.some((x) => name.includes(x))) tests.push({ ...test, name });
  }
}

console.log(`exe     ${exe}\nprofile ${root}\ntests   ${tests.length}\n`);
const results = [];
try {
  await launch();
  for (const test of tests) {
    const started = Date.now();
    pageErrors = [];
    foregrounded = false;
    try {
      if (!proc || proc.exitCode !== null) await launch();
      await reset();
      await test.run(t);
      if (pageErrors.length) throw new Error(`passed, but left ${pageErrors.length} page error(s)`);
      results.push({ name: test.name, ok: true, ms: Date.now() - started });
      console.log(`  pass  ${test.name} (${Date.now() - started} ms)`);
    } catch (e) {
      if (e instanceof Skip) {
        results.push({ name: test.name, ok: true, skipped: true, ms: Date.now() - started });
        console.log(`  skip  ${test.name}: ${e.message}`);
        continue;
      }
      results.push({ name: test.name, ok: false, ms: Date.now() - started, error: e.message });
      console.log(`  FAIL  ${test.name}\n        ${String(e.message).split("\n").join("\n        ")}`);
      const status = await cdp?.ev("__e2e.app.status").catch(() => "");
      if (status) console.log(`        app status: ${status}`);
      // The message and the top of its stack.
      for (const err of pageErrors.slice(0, 5)) console.log(`        page error: ${String(err).split("\n").slice(0, 3).join("\n          ")}`);
      await t.screenshot(test.name.replace(/[^a-z0-9]+/gi, "-")).catch(() => {});
      // A dialog, menu or prompt the test left open would break the next test.
      await quit();
    }
  }
} finally {
  await quit();
  // Put back anything a failed test left in the Recycle Bin. Only items whose
  // name carries this run's token can match.
  await t.ps("recycle-restore.ps1", "-Match", token).catch((e) => console.log(`bin cleanup: ${e.message}`));
}

const failed = results.filter((r) => !r.ok);
const skipped = results.filter((r) => r.skipped).length;
console.log(`\n${results.length - failed.length - skipped} passed, ${failed.length} failed${skipped ? `, ${skipped} skipped` : ""}`);
if (has("--keep") || failed.length) console.log(`kept ${root}`);
else fs.rmSync(root, { recursive: true, force: true });
process.exit(failed.length ? 1 : 0);
