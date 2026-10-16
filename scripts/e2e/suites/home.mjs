// Home (F11): Quick access pins, Favorites and Recent files. The runner points
// %APPDATA% at its temp folder, so the Recent items and lists here are the
// test profile's own; the user's are never read or changed. Mouse input is
// sent through DevTools only, never the real mouse.
import fs from "node:fs";
import path from "node:path";

/** Windows' Recent items folder of the test profile. */
const recentDir = (t) => path.join(t.root, "appdata", "Microsoft", "Windows", "Recent");

/** A shortcut in Recent items pointing at `target`, as Windows makes when a file is opened. */
async function addRecent(t, target) {
  const lnk = path.join(recentDir(t), `${path.basename(target)}.lnk`);
  fs.mkdirSync(path.dirname(lnk), { recursive: true });
  await t.psCmd(`$s = (New-Object -ComObject WScript.Shell).CreateShortcut('${lnk}'); $s.TargetPath = '${target}'; $s.Save()`);
  return lnk;
}

/** Row names in one Home list ("Favorites" or "Recent files"). */
const rowsOf = (t, list) => t.ev(`[...document.querySelectorAll('.homepane [aria-label="${list}"] .rname')].map((r) => r.textContent)`);

/** Right-click an element (DevTools input) and pick an entry of the menu that opens. */
async function pick(t, selector, label) {
  await t.click(selector, { button: "right" });
  await t.waitFor(`!!document.querySelector(".menu")`, 3000, `a menu for ${selector}`);
  await t.click(".menu .menu-item", { where: { has: label } });
}

/** A left-button drag from one point to another, in DevTools mouse events. */
async function drag(t, from, to) {
  const m = (type, p, extra) => t.send("Input.dispatchMouseEvent", { type, x: p.x, y: p.y, ...extra });
  await m("mouseMoved", from, { button: "none" });
  await m("mousePressed", from, { button: "left", buttons: 1, clickCount: 1 });
  for (let i = 1; i <= 10; i++) await m("mouseMoved", { x: from.x + ((to.x - from.x) * i) / 10, y: from.y + ((to.y - from.y) * i) / 10 }, { button: "left", buttons: 1 });
  await m("mouseReleased", to, { button: "left", buttons: 0, clickCount: 1 });
}

export default [
  {
    name: "Remove from Recent takes off only that file, and Clear recent files removes every entry and nothing else",
    async run(t) {
      const d = t.dir("home-recent");
      const a = t.mk(path.join(d, "alpha.txt"), "a");
      const b = t.mk(path.join(d, "beta.txt"), "b");
      const lnkA = await addRecent(t, a);
      const lnkB = await addRecent(t, b);
      // What Clear must leave alone: other apps' jump lists, and anything not a shortcut.
      const jump = t.mk(path.join(recentDir(t), "AutomaticDestinations", "x.automaticDestinations-ms"), "j");
      const other = t.mk(path.join(recentDir(t), "notes.ini"), "n");
      await t.nav("::Home");
      await t.until(async () => (await rowsOf(t, "Recent files")).length === 2, 5000, "both files in Recent files");
      await pick(t, `.homepane [aria-label="Recent files"] .rrow[title="${a.replace(/\\/g, "\\\\")}"]`, "Remove from Recent");
      await t.until(async () => JSON.stringify(await rowsOf(t, "Recent files")) === JSON.stringify(["beta.txt"]), 5000, "only beta.txt left");
      t.ok(!fs.existsSync(lnkA) && fs.existsSync(lnkB), "alpha's shortcut went, beta's stayed");
      t.ok(fs.existsSync(a), "the file itself stayed");
      t.eq(await t.call("clearRecent"), 1, "Clear removed one entry");
      t.ok(!fs.existsSync(lnkB), "beta's shortcut went");
      t.ok(fs.existsSync(jump) && fs.existsSync(other) && fs.existsSync(b), "jump lists, other files and the files themselves stayed");
      t.eq((await rowsOf(t, "Recent files")).length, 0, "Home's list is empty");
    },
  },
  {
    name: "Add to Favorites puts a file on Home, where it stays after a restart, and Remove takes it off",
    async run(t) {
      const d = t.dir("home-fav");
      const f = t.mk(path.join(d, "keep me.txt"), "x");
      await t.nav(d);
      await t.select(["keep me.txt"]);
      await t.ev("__e2e.app.focus = 0");
      await t.ev("__e2e.hooks.contextMenu()");
      await t.waitFor(`!!document.querySelector(".ctxmenu")`, 3000, "the item menu");
      await t.click(".ctxmenu .menu-item", { where: { has: "Add to Favorites" } });
      await t.waitFor(`__e2e.app.favorites.length === 1`, 3000, "in Favorites");
      await t.restart();
      await t.nav("::Home");
      t.eq(await rowsOf(t, "Favorites"), ["keep me.txt"], "on Home after a restart");
      t.eq(await t.ev(`document.querySelector('.homepane [aria-label="Favorites"] .rpath').textContent`), d, "with the folder it's in");
      await pick(t, `.homepane [aria-label="Favorites"] .rrow`, "Remove from Favorites");
      await t.waitFor(`__e2e.app.favorites.length === 0`, 3000, "off the list");
      t.ok(fs.existsSync(f), "the file stayed");
      t.ok(await t.ev(`!!document.querySelector(".homepane .empty")?.textContent.includes("Files you add to Favorites")`), "Home says how to add one");
    },
  },
  {
    name: "pins are reordered by dragging, on Home and in the navigation pane, and keep their order",
    async run(t) {
      const pins = ["one", "two", "three"].map((n) => t.dir(`home-pins/${n}`));
      const before = await t.ev("__e2e.app.pins.slice()");
      try {
        for (const p of pins) await t.call("pinAdd", p);
        await t.nav("::Home");
        const tile = (i) => t.center(`.homepane [data-pin="${before.length + i}"]`);
        await drag(t, await tile(0), await tile(2));
        t.eq((await t.ev("__e2e.app.pins.slice()")).slice(before.length), [pins[1], pins[2], pins[0]], "one went last on Home");
        const saved = JSON.parse(fs.readFileSync(path.join(t.root, "appdata", "Filezy", "pins.json"), "utf8"));
        t.eq(saved.slice(before.length), [pins[1], pins[2], pins[0]], "and that's the order saved");
        // In the navigation pane, dragging doesn't open the pin it started on.
        const nav = (i) => t.center(`.navpane [data-pin="${before.length + i}"]`);
        await drag(t, await nav(2), await nav(0));
        t.eq((await t.ev("__e2e.app.pins.slice()")).slice(before.length), [pins[0], pins[1], pins[2]], "back to the first order in the pane");
        t.eq(await t.ev("__e2e.app.path"), "::Home", "the drag opened nothing");
      } finally {
        for (const p of pins) await t.call("pinRemove", p);
      }
    },
  },
  {
    name: "Home says where each Quick access folder is: the cloud folder's name inside one, else the folder holding it",
    async run(t) {
      const cloud = t.dir("home-where/Cloudy");
      const inside = t.dir("home-where/Cloudy/Work");
      const outside = t.dir("home-where/Plain");
      const roots = await t.ev("__e2e.app.cloudRoots.slice()");
      try {
        await t.ev(`__e2e.app.cloudRoots = [...__e2e.app.cloudRoots, { path: ${JSON.stringify(cloud)}, name: "Test Cloud" }]`);
        await t.call("pinAdd", inside);
        await t.call("pinAdd", outside);
        await t.nav("::Home");
        const sub = (p) => t.ev(`document.querySelector(${JSON.stringify(`.homepane .qtile[title="${p.replace(/\\/g, "\\\\")}"] .qsub`)})?.textContent`);
        t.eq(await sub(inside), "Test Cloud", "a folder in a cloud folder");
        t.eq(await sub(outside), path.dirname(outside), "a local folder");
      } finally {
        await t.call("pinRemove", inside);
        await t.call("pinRemove", outside);
        await t.ev(`__e2e.app.cloudRoots = ${JSON.stringify(roots)}`);
      }
    },
  },
];
