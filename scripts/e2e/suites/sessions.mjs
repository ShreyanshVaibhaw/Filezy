// Tabs, windows and launches (F11). Everything goes through the page and the
// file system; no real mouse or keyboard input is sent.
import fs from "node:fs";
import path from "node:path";

const tabIds = (t) => t.ev("__e2e.tabs.map((x) => x.id)");
const tabPaths = (t) => t.ev("__e2e.tabs.map((x) => x.path.toLowerCase())");
const low = (p) => p.toLowerCase();

/** The window's outer bounds in physical pixels, and whether it's maximized. */
const bounds = (w) =>
  w.ev("Promise.all([__e2e.win.outerPosition(), __e2e.win.outerSize(), __e2e.win.isMaximized()]).then(([p, s, m]) => ({ x: p.x, y: p.y, w: s.width, h: s.height, max: m }))");

/** Wait until the window is (or isn't) maximized and has stopped moving; returns its bounds. */
async function settledBounds(t, max) {
  await t.waitFor(`__e2e.win.isMaximized().then((m) => m === ${max})`, 5000, max ? "maximized" : "restored");
  let last = await bounds(t);
  for (let i = 0; i < 50; i++) {
    await t.sleep(50);
    const now = await bounds(t);
    if (JSON.stringify(now) === JSON.stringify(last)) return now;
    last = now;
  }
  throw new Error(`the window kept moving: ${JSON.stringify(last)}`);
}

export default [
  {
    name: "a tab whose folder is gone opens the nearest folder that still exists and says so; other tabs are untouched",
    async run(t) {
      const d = t.dir("ses-gone");
      const keep = t.dir("ses-gone/keep");
      const gone = t.dir("ses-gone/parent/gone");
      t.mk(path.join(keep, "here.txt"), "x");
      await t.nav(keep);
      const [first] = await tabIds(t);
      await t.call("newTab", gone);
      await t.settle();
      const second = (await tabIds(t))[1];
      await t.call("switchTab", first);
      await t.settle();
      fs.rmSync(gone, { recursive: true });
      await t.call("switchTab", second);
      await t.waitFor(`__e2e.app.path.toLowerCase() === ${JSON.stringify(path.join(d, "parent").toLowerCase())}`, 5000, "the tab opened the gone folder's parent");
      t.eq((await t.names()).length, 0, "and shows that folder's items, not the other tab's");
      t.eq(await t.ev("__e2e.tabs.find((x) => x.id === __e2e.app.activeTab).path.toLowerCase()"), path.join(d, "parent").toLowerCase(), "the tab itself moved there");
      t.ok((await t.ev("__e2e.app.status")).includes("gone"), "the message bar says which folder is gone");
      await t.call("switchTab", first);
      await t.settle();
      t.eq(await t.names(), ["here.txt"], "the other tab still shows its folder");
    },
  },
  {
    name: "with Reopen tabs on, a restart brings back every tab and the active one; off, one tab opens on the start page",
    async run(t) {
      const a = t.dir("ses-restore/a");
      const b = t.dir("ses-restore/b");
      const gone = t.dir("ses-restore/c/gone");
      await t.call("setSetting", "restoreTabs", true);
      try {
        await t.nav(a);
        await t.call("newTab", b);
        await t.call("newTab", gone);
        await t.settle();
        await t.call("switchTab", (await tabIds(t))[1]);
        await t.settle();
        fs.rmSync(gone, { recursive: true });
        await t.restart();
        t.eq(await tabPaths(t), [a, b, gone].map(low), "every tab came back, in order");
        t.eq(low(await t.ev("__e2e.app.path")), low(b), "the active one too, listed");
        // The one whose folder went meanwhile opens the nearest folder that exists.
        await t.call("switchTab", (await tabIds(t))[2]);
        await t.waitFor(`__e2e.app.path.toLowerCase() === ${JSON.stringify(low(path.dirname(gone)))}`, 5000, "the gone folder's tab opened its parent");
        await t.call("setSetting", "restoreTabs", false);
        await t.restart();
        t.eq(await t.ev("__e2e.tabs.length"), 1, "one tab");
        t.eq(await t.ev("__e2e.app.path"), "::Home", "on the start page");
      } finally {
        await t.call("setSetting", "restoreTabs", false);
      }
    },
  },
  {
    name: "a damaged record of the last tabs is ignored, and the start page opens",
    async run(t) {
      await t.call("setSetting", "restoreTabs", true);
      try {
        // The record is only rewritten when tabs change, so a damaged one
        // written just before closing is what the next start reads.
        for (const bad of ["{not json", JSON.stringify({ v: 99, tabs: ["C:\\"], active: 0 }), JSON.stringify({ v: 1, tabs: [5], active: 0 })]) {
          await t.ev(`localStorage.setItem("session", ${JSON.stringify(bad)})`);
          await t.restart();
          t.eq(await t.ev("__e2e.tabs.length"), 1, `one tab for ${bad}`);
          t.eq(await t.ev("__e2e.app.path"), "::Home", `the start page for ${bad}`);
        }
      } finally {
        await t.call("setSetting", "restoreTabs", false);
      }
    },
  },
  {
    name: "the window opens where and as big as it closed, maximized if it was, and a second window opens a little apart",
    async run(t) {
      try {
        await t.ev("__e2e.win.unmaximize()");
        await t.ev("__e2e.win.setSize(new __e2e.LogicalSize(900, 640))");
        await t.ev("__e2e.win.center()");
        const before = await settledBounds(t, false);
        await t.restart();
        const after = await bounds(t);
        t.eq([after.x, after.y, after.w, after.h, after.max], [before.x, before.y, before.w, before.h, false], "same place and size");
        // A second window doesn't cover the first exactly.
        await t.ev(`__e2e.invoke("new_window", { path: __e2e.app.path })`);
        const w = await t.otherWindow();
        try {
          const second = await bounds(w);
          t.ok(second.x > after.x && second.y > after.y && second.w === after.w, `cascaded: ${JSON.stringify([after, second])}`);
        } finally {
          await w.ev("void __e2e.closeTab(__e2e.app.activeTab)").catch(() => {});
          w.close();
        }
        await t.ev("__e2e.win.maximize()");
        await settledBounds(t, true);
        await t.restart();
        t.ok((await bounds(t)).max, "maximized again");
        await t.ev("__e2e.win.unmaximize()");
        const normal = await settledBounds(t, false);
        t.eq([normal.w, normal.h], [after.w, after.h], "and its normal size is the one it had");
      } finally {
        // Leave the default size for the suites that follow.
        await t.ev("__e2e.win.unmaximize()").catch(() => {});
        await t.ev("__e2e.win.setSize(new __e2e.LogicalSize(1280, 820))").catch(() => {});
        await t.ev("__e2e.win.center()").catch(() => {});
      }
    },
  },
  {
    name: "Move to new window opens the tab's folder in another window and closes it here",
    async run(t) {
      const a = t.dir("ses-move/a");
      const b = t.dir("ses-move/b");
      await t.nav(a);
      await t.call("newTab", b);
      await t.settle();
      const moved = (await tabIds(t))[1];
      await t.call("transferTab", moved);
      const w = await t.otherWindow();
      try {
        t.eq(low(await w.ev("__e2e.app.path")), low(b), "the other window shows the tab's folder");
        t.eq(await tabPaths(t), [low(a)], "and the tab is gone from here");
      } finally {
        await w.ev("void __e2e.closeTab(__e2e.app.activeTab)").catch(() => {});
        w.close();
      }
    },
  },
  {
    name: "Open in Filezy can be added to Explorer's folder menus from Settings and taken out again",
    async run(t) {
      // A test build writes under a scratch key of its own (main.rs classes_root),
      // never the user's Software\Classes.
      const scratch = "HKCU\\Software\\FilezyE2E\\Classes";
      const cmd = (place) => t.psCmd(`(Get-ItemProperty -LiteralPath 'Registry::${scratch}\\${place}\\shell\\Filezy\\command' -ErrorAction SilentlyContinue).'(default)'`);
      const toggle = `button[role="switch"][aria-label="Open in Filezy from File Explorer"]`;
      try {
        await t.nav("::Settings");
        await t.click(".railitem", { where: { has: "Files and folders" } });
        await t.waitFor(`document.querySelector(${JSON.stringify(toggle)})?.getAttribute("aria-checked") === "false"`, 3000, "off at first");
        // It's further down the page than the window shows.
        await t.ev(`document.querySelector(${JSON.stringify(toggle)}).scrollIntoView({ block: "center" })`);
        await t.click(toggle);
        await t.waitFor(`document.querySelector(${JSON.stringify(toggle)})?.getAttribute("aria-checked") === "true"`, 3000, "on");
        for (const place of ["Directory", "Directory\\Background", "Drive"]) {
          const c = (await cmd(place)).trim();
          t.ok(/^"[^"]+\\filezy\.exe" "%V"$/i.test(c), `${place} opens this copy with the folder: ${c}`);
        }
        t.eq((await t.psCmd(`Test-Path 'Registry::HKCU\\Software\\Classes\\Directory\\shell\\Filezy'`)).trim(), "False", "the user's own classes weren't touched");
        await t.click(toggle);
        await t.waitFor(`document.querySelector(${JSON.stringify(toggle)})?.getAttribute("aria-checked") === "false"`, 3000, "off again");
        for (const place of ["Directory", "Directory\\Background", "Drive"]) t.eq((await cmd(place)).trim(), "", `${place} entry gone`);
      } finally {
        await t.psCmd(`if (Test-Path 'Registry::HKCU\\Software\\FilezyE2E') { Remove-Item -LiteralPath 'Registry::HKCU\\Software\\FilezyE2E' -Recurse }`);
        await t.nav("::Home");
      }
    },
  },
  {
    name: "launching with a folder or a quoted /select, whose names have spaces and other scripts opens it",
    async run(t) {
      const d = t.dir("ses launch 文件 ü");
      t.mk(path.join(d, "a.txt"), "a");
      const f = t.mk(path.join(d, "the chosen 選択.txt"), "b");
      try {
        await t.restart([d]);
        await t.settle();
        t.eq(await t.ev("__e2e.app.path"), d, "the folder");
        // Explorer's own form: the path quoted after the comma.
        await t.restart([`/select,"${f}"`]);
        await t.settle();
        t.eq(await t.ev("__e2e.app.path"), d, "the file's folder");
        t.eq(await t.selectedNames(), ["the chosen 選択.txt"], "with the file selected");
      } finally {
        await t.restart();
      }
    },
  },
  {
    name: "dual pane puts this window and a new one side by side, and Copy to the other window copies there",
    async run(t) {
      const a = t.dir("ses-dual/a");
      const b = t.dir("ses-dual/b");
      t.mk(path.join(a, "file.txt"), "x");
      await t.nav(a);
      await t.ev(`__e2e.invoke("split_view", { path: ${JSON.stringify(b)} })`);
      const w = await t.otherWindow();
      try {
        t.eq(low(await w.ev("__e2e.app.path")), low(b), "the new window shows its folder");
        const rect = (x) => x.ev("Promise.all([__e2e.win.outerPosition(), __e2e.win.outerSize()]).then(([p, s]) => [p.x, p.y, s.width, s.height])");
        const [l, r] = [await rect(t), await rect(w)];
        t.ok(l[0] < r[0] && Math.abs(l[1] - r[1]) <= 2 && Math.abs(l[3] - r[3]) <= 2, `side by side: ${l} and ${r}`);
        await t.select(["file.txt"]);
        await t.call("sendSelTo", b, false);
        await t.until(() => fs.existsSync(path.join(b, "file.txt")), 10000, "copied into the other window's folder");
        t.ok(fs.existsSync(path.join(a, "file.txt")), "and the original stays");
      } finally {
        await w.ev("void __e2e.closeTab(__e2e.app.activeTab)").catch(() => {});
        w.close();
      }
    },
  },
];
