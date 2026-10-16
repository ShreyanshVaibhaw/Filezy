// Drag and drop from other apps (real OLE drags, see drag.ps1) and clipboard
// changes made by other apps.
import fs from "node:fs";
import path from "node:path";

const NONE = 0;
const COPY = 1;
const MOVE = 2;
const LINK = 4;
const DROPPED = 0x40100;

/** "C:\x" -> "\\localhost\C$\x": the same file through the loopback share, a different volume. */
const unc = (p) => `\\\\localhost\\${p[0].toUpperCase()}$${p.slice(2)}`;

const settleOp = async (t) => {
  await t.sleep(400);
  await t.settle();
};

const rect = (t, sel) =>
  t.ev(`(() => {
    const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect();
    return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`);

/** The middle of a file list row, by name. */
const rowAt = (t, name) =>
  t.ev(`(() => {
    const row = [...document.querySelectorAll(".vrow")].find((r) => r.querySelector(".fname")?.textContent === ${JSON.stringify(name)});
    const b = row.getBoundingClientRect();
    return { x: b.left + 120, y: b.top + b.height / 2 };
  })()`);

/** A blank spot in the file list, below its rows. */
const blank = async (t) => {
  const r = await rect(t, ".scroller");
  return { x: r.left + 240, y: r.bottom - 40 };
};

/** Every drag starts over the Back button, which is on every page and takes no drops. */
const drop = async (t, files, at, opts = {}) => {
  const s = await rect(t, ".navbtn");
  return t.drag(files, [{ x: s.x, y: s.y }, { ...at, ms: opts.ms ?? 300 }], opts);
};

/** Set the clipboard from another process until it sticks (another program here writes it too). */
async function clipboard(t, args, cond) {
  for (let i = 0; ; i++) {
    await t.ps("clip.ps1", ...args);
    try {
      return await t.waitFor(cond, 3000, `clipboard ${args.join(" ")}`);
    } catch (e) {
      if (i >= 3) throw e;
    }
  }
}

export default [
  {
    name: "a file dragged in from another app moves on the same volume; the cursor says Move and undo puts it back",
    async run(t) {
      const src = t.dir("dd-move/src");
      const dest = t.dir("dd-move/dest");
      const f = t.mk(path.join(src, "moved.txt"), "m");
      await t.nav(dest);
      const r = await drop(t, [f], await blank(t));
      t.eq(r.hr, DROPPED, "dropped");
      t.eq(r.feedback.at(-1), MOVE, "the cursor showed Move");
      t.eq([r.effect, r.performed, r.logical], [NONE, NONE, MOVE], "an optimized move: the source deletes nothing itself");
      await settleOp(t);
      t.eq([fs.readdirSync(src), fs.readdirSync(dest)], [[], ["moved.txt"]], "moved");
      t.eq(await t.selectedNames(), ["moved.txt"], "the dropped item is selected");
      await t.key("z", { ctrl: true });
      await settleOp(t);
      t.eq(fs.readdirSync(src), ["moved.txt"], "undo moved it back");
    },
  },
  {
    name: "Ctrl copies, Alt makes a shortcut, and another volume copies unless Shift is held",
    async run(t) {
      const src = t.dir("dd-keys/src");
      const dest = t.dir("dd-keys/dest");
      const f = t.mk(path.join(src, "doc.txt"), "d");
      await t.nav(dest);
      let r = await drop(t, [f], await blank(t), { keys: "ctrl" });
      t.eq([r.feedback.at(-1), r.effect], [COPY, COPY], "Ctrl shows and does Copy");
      await settleOp(t);
      t.ok(t.exists(f) && t.exists(path.join(dest, "doc.txt")), "copied");
      r = await drop(t, [f], await blank(t), { keys: "alt" });
      t.eq(r.feedback.at(-1), LINK, "Alt shows Link");
      await settleOp(t);
      t.ok(t.exists(path.join(dest, "doc.txt - Shortcut.lnk")), "a shortcut");
      const far = unc(t.mk(path.join(src, "far.txt"), "f"));
      r = await drop(t, [far], await blank(t));
      t.eq(r.feedback.at(-1), COPY, "another volume copies");
      await settleOp(t);
      t.ok(t.exists(path.join(src, "far.txt")) && t.exists(path.join(dest, "far.txt")), "copied across");
      const far2 = unc(t.mk(path.join(src, "far2.txt"), "g"));
      r = await drop(t, [far2], await blank(t), { keys: "shift" });
      t.eq(r.feedback.at(-1), MOVE, "Shift moves across volumes");
      await settleOp(t);
      t.ok(!t.exists(path.join(src, "far2.txt")) && t.exists(path.join(dest, "far2.txt")), "moved across");
    },
  },
  {
    name: "pressing Ctrl mid-drag turns Move into Copy, and a folder on a network share takes drops",
    async run(t) {
      const src = t.dir("dd-mid/src");
      const dest = t.dir("dd-mid/dest");
      const f = t.mk(path.join(src, "mid.txt"), "m");
      await t.nav(dest);
      const at = await blank(t);
      const s = await rect(t, ".navbtn");
      const r = await t.drag([f], [{ x: s.x, y: s.y }, { ...at, ms: 300 }, { ...at, ms: 300, keys: "ctrl" }]);
      t.eq(r.feedback, [NONE, MOVE, COPY], "Move, then Copy once Ctrl went down");
      t.eq(r.effect, COPY, "dropped as a copy");
      await settleOp(t);
      t.ok(t.exists(f) && t.exists(path.join(dest, "mid.txt")), "copied");
      const share = t.dir("dd-mid/share");
      await t.nav(unc(share));
      const g = t.mk(path.join(src, "net.txt"), "n");
      const r2 = await drop(t, [g], await blank(t));
      t.eq(r2.feedback.at(-1), COPY, "a share is another volume, so Copy");
      await settleOp(t);
      t.ok(t.exists(g) && t.exists(path.join(share, "net.txt")), "copied onto the share");
    },
  },
  {
    name: "a drag resting on a collapsed folder in the navigation pane expands it",
    async run(t) {
      const f = t.mk(path.join(t.dir("dd-expand"), "e.txt"), "e");
      const drive = path.parse(t.root).root;
      const node = `[...document.querySelectorAll(".navitem")].find((b) => b.title === ${JSON.stringify(drive)})`;
      t.eq(await t.ev(`${node}.getAttribute("aria-expanded")`), "false", "collapsed at first");
      // Cancelled: nothing may ever land in a drive's root.
      await drop(t, [f], await t.center(".navitem", { title: drive }), { ms: 1500, cancel: true });
      t.eq(await t.ev(`${node}.getAttribute("aria-expanded")`), "true", "expanded while the drag rested on it");
      t.ok(t.exists(f), "nothing dropped");
      await t.ev(`${node}.querySelector(".chev").dispatchEvent(new MouseEvent("mousedown", { bubbles: true }))`);
    },
  },
  {
    name: "Ctrl+drop into an item's own folder makes a copy; a plain drop there does nothing",
    async run(t) {
      const d = t.dir("dd-same");
      const f = t.mk(path.join(d, "a.txt"), "a");
      await t.nav(d);
      let r = await drop(t, [f], await blank(t));
      t.eq(r.feedback, [NONE], "nothing to do");
      r = await drop(t, [f], await blank(t), { keys: "ctrl" });
      t.eq(r.feedback.at(-1), COPY, "Ctrl shows Copy");
      await settleOp(t);
      t.eq(fs.readdirSync(d).sort(), ["a - Copy.txt", "a.txt"], "Explorer's copy name");
    },
  },
  {
    name: "folder rows, breadcrumbs, the navigation pane, Home tiles and tabs take drops; resting on a tab switches to it",
    async run(t) {
      const src = t.dir("dd-targets/src");
      const inner = t.dir("dd-targets/dest/inner");
      const dest = path.dirname(inner);
      const other = t.dir("dd-targets/other");
      const pinned = t.dir("dd-targets/pinned");
      const mk = (n) => t.mk(path.join(src, n), n);
      await t.nav(dest);
      await drop(t, [mk("row.txt")], await rowAt(t, "inner"));
      await settleOp(t);
      t.ok(t.exists(path.join(inner, "row.txt")), "folder row");
      await t.nav(inner);
      await drop(t, [mk("crumb.txt")], await t.center(".crumb", { text: "dest" }));
      await settleOp(t);
      t.ok(t.exists(path.join(dest, "crumb.txt")), "breadcrumb");
      await t.call("pinAdd", pinned);
      try {
        await drop(t, [mk("pin.txt")], await t.center(".navitem", { title: pinned }));
        await settleOp(t);
        t.ok(t.exists(path.join(pinned, "pin.txt")), "navigation pane");
        await t.nav("::Home");
        await drop(t, [mk("home.txt")], await t.center(".qtile", { title: pinned }));
        await settleOp(t);
        t.ok(t.exists(path.join(pinned, "home.txt")), "Home tile");
      } finally {
        await t.call("pinRemove", pinned);
      }
      await t.nav(dest);
      await t.call("newTab", other);
      await t.settle();
      const [first, second] = await t.ev("__e2e.tabs.map((x) => x.id)");
      await t.call("switchTab", first);
      await t.settle();
      await drop(t, [mk("tab.txt")], await t.center(`.tab[data-tab="${second}"]`), { ms: 1500 });
      await settleOp(t);
      t.eq(await t.ev("__e2e.app.activeTab"), second, "the tab switched while the drag rested on it");
      t.ok(t.exists(path.join(other, "tab.txt")), "tab");
    },
  },
  {
    name: "dropping on the Recycle Bin recycles",
    async run(t) {
      const d = t.dir("dd-bin");
      const f = t.mk(path.join(d, `bin-${t.token}.txt`), "b");
      await t.nav(d);
      const r = await drop(t, [f], await t.center(".navitem", { has: "Recycle Bin" }));
      t.eq(r.feedback.at(-1), MOVE, "Move to Recycle Bin");
      await settleOp(t);
      t.ok(!t.exists(f), "gone from the folder");
      await t.ps("recycle-restore.ps1", "-Match", t.token);
      t.ok(t.exists(f), "and back from the Recycle Bin");
    },
  },
  {
    name: "Home, This PC, search results and a folder itself refuse a drop",
    async run(t) {
      const d = t.dir("dd-refuse");
      const sub = t.dir("dd-refuse/sub");
      const f = t.mk(path.join(t.dir("dd-refuse-src"), "here.txt"), "h");
      const corner = async (sel) => {
        const r = await rect(t, sel);
        return { x: r.right - 40, y: r.bottom - 20 };
      };
      await t.nav("::Home");
      t.eq((await drop(t, [f], await corner(".homepane"))).feedback, [NONE], "Home");
      await t.nav("::ThisPC");
      t.eq((await drop(t, [f], await corner(".drivespane"))).feedback, [NONE], "This PC");
      // A drive tile is a destination (cancelled: nothing may land in a drive's root).
      const drive = path.parse(t.root).root;
      const r = await drop(t, [f], await t.center(".drivetile", { title: drive }), { cancel: true });
      t.eq(r.feedback.at(-1), MOVE, "a drive tile on the same volume shows Move");
      await t.nav(d);
      t.eq((await drop(t, [sub], await rowAt(t, "sub"))).feedback, [NONE], "a folder into itself");
      await t.call("startSearch", "sub");
      await t.waitFor("__e2e.app.search?.done", 10000, "search");
      await t.settle();
      t.eq((await drop(t, [f], await blank(t))).feedback, [NONE], "search results");
      t.ok(t.exists(f) && fs.readdirSync(sub).length === 0, "nothing moved");
    },
  },
  {
    name: "a right-drag ends in Explorer's menu: Copy here copies, Cancel does nothing",
    async run(t) {
      const src = t.dir("dd-right/src");
      const dest = t.dir("dd-right/dest");
      const f = t.mk(path.join(src, "r.txt"), "r");
      await t.nav(dest);
      // Entries are clicked with the real mouse. Keys pressed in this menu
      // (another process is the drag's source) sometimes don't reach it; the
      // same menu from a drag inside Filezy takes keys (see below).
      let dragging = drop(t, [f], await blank(t), { right: true });
      await t.uia("-Menu", "-Pick", "Copy here");
      let r = await dragging;
      t.eq(r.effect, COPY, "the menu's choice");
      await settleOp(t);
      t.ok(t.exists(f) && t.exists(path.join(dest, "r.txt")), "copied");
      const g = t.mk(path.join(src, "s.txt"), "s");
      dragging = drop(t, [g], await blank(t), { right: true });
      await t.uia("-Menu", "-Pick", "Cancel");
      r = await dragging;
      t.eq(r.effect, NONE, "cancelled");
      await settleOp(t);
      t.ok(t.exists(g) && !t.exists(path.join(dest, "s.txt")), "Cancel left it alone");
      // The keyboard is back in the list: a real B selects by name.
      t.mk(path.join(dest, "banana.txt"), "b");
      await settleOp(t);
      await t.ev("document.querySelector('.scroller').focus()");
      await t.uia("-Press", String(0x42));
      await t.waitFor(`__e2e.selectedNames().then((n) => n.join() === "banana.txt")`, 3000, "type-to-select after the menu");
    },
  },
  {
    name: "a drag resting at the bottom of a long list scrolls it, and a cancelled drag leaves nothing behind",
    async run(t) {
      const d = t.dir("dd-scroll");
      for (let i = 0; i < 80; i++) t.mk(path.join(d, `f${String(i).padStart(3, "0")}.txt`));
      const f = t.mk(path.join(t.dir("dd-scroll-src"), "x.txt"), "x");
      await t.nav(d);
      const s = await rect(t, ".scroller");
      const r = await drop(t, [f], { x: s.left + 240, y: s.bottom - 6 }, { ms: 1500, cancel: true });
      t.eq(r.feedback.at(-1), MOVE, "a move into this folder while dragging");
      t.ok((await t.ev("document.querySelector('.scroller').scrollTop")) > 0, "the list scrolled");
      t.eq(await t.ev("document.querySelectorAll('[data-drop-on]').length"), 0, "no highlight left");
      t.ok(t.exists(f) && !t.exists(path.join(d, "x.txt")), "nothing dropped");
    },
  },
  {
    name: "dragging rows onto a folder in the same list moves them, and undo puts them back",
    async run(t) {
      const d = t.dir("dd-own");
      const sub = t.dir("dd-own/sub");
      for (const n of ["a.txt", "b.txt", "c.txt"]) t.mk(path.join(d, n), n);
      await t.nav(d);
      await t.select(["a.txt", "b.txt"]);
      const from = await rowAt(t, "a.txt");
      await t.mouseDrag([from, { x: from.x + 30, y: from.y + 10, ms: 300 }, { ...(await rowAt(t, "sub")), ms: 500 }]);
      await settleOp(t);
      t.eq([fs.readdirSync(d).sort(), fs.readdirSync(sub).sort()], [["c.txt", "sub"], ["a.txt", "b.txt"]], "both selected items moved");
      await t.key("z", { ctrl: true });
      await settleOp(t);
      t.eq(fs.readdirSync(d).sort(), ["a.txt", "b.txt", "c.txt", "sub"], "undo moved them back");
    },
  },
  {
    name: "Ctrl held from the start copies the whole selection, and onto the list itself gives Explorer's copy names",
    async run(t) {
      const d = t.dir("dd-own-ctrl");
      const sub = t.dir("dd-own-ctrl/sub");
      for (const n of ["a.txt", "b.txt"]) t.mk(path.join(d, n), n);
      await t.nav(d);
      await t.select(["a.txt", "b.txt"]);
      // Ctrl+press on a selected item keeps the selection for the drag.
      let from = await rowAt(t, "b.txt");
      await t.mouseDrag([from, { x: from.x + 30, y: from.y + 10, ms: 300 }, { ...(await rowAt(t, "sub")), ms: 500 }], { ctrl: true });
      await settleOp(t);
      t.eq(fs.readdirSync(sub).sort(), ["a.txt", "b.txt"], "both copied into the folder");
      t.ok(t.exists(path.join(d, "a.txt")) && t.exists(path.join(d, "b.txt")), "and still here");
      await t.select(["a.txt"]);
      from = await rowAt(t, "a.txt");
      await t.mouseDrag([from, { x: from.x + 30, y: from.y + 10, ms: 300 }, { ...(await blank(t)), ms: 500 }], { ctrl: true });
      await settleOp(t);
      t.ok(t.exists(path.join(d, "a - Copy.txt")), "a copy in the same folder");
    },
  },
  {
    name: "a right-drag from the list ends in the same menu",
    async run(t) {
      const d = t.dir("dd-own-right");
      const sub = t.dir("dd-own-right/sub");
      t.mk(path.join(d, "r.txt"), "r");
      await t.nav(d);
      await t.select(["r.txt"]);
      const from = await rowAt(t, "r.txt");
      const dragging = t.mouseDrag([from, { x: from.x + 30, y: from.y + 10, ms: 300 }, { ...(await rowAt(t, "sub")), ms: 500 }], { right: true });
      await t.uia("-Menu", "-Key", String(0x43));
      await dragging;
      await settleOp(t);
      t.ok(t.exists(path.join(d, "r.txt")) && t.exists(path.join(sub, "r.txt")), "Copy here copied");
      t.eq(await t.ev("!!document.querySelector('.ctxmenu')"), false, "the item menu didn't open as well");
    },
  },
  {
    name: "another app's copy turns Paste on at once, and its cut ghosts those items until the clipboard changes",
    async run(t) {
      const d = t.dir("clip-watch");
      const f = t.mk(path.join(d, "cutme.txt"), "c");
      t.mk(path.join(d, "other.txt"), "o");
      await t.nav(d);
      await clipboard(t, ["-Text", "no files here"], "!__e2e.app.canPaste");
      await clipboard(t, ["-Set", f, "-Cut"], "__e2e.app.canPaste && __e2e.app.cut.size === 1");
      t.eq(await t.ev(`[...document.querySelectorAll(".vrow.cut .fname")].map((e) => e.textContent)`), ["cutme.txt"], "only the cut item is ghosted");
      await clipboard(t, ["-Text", "something else"], "__e2e.app.cut.size === 0 && !document.querySelector('.vrow.cut')");
    },
  },
  {
    name: "Explorer's Paste takes Filezy's copy, and Filezy pastes Explorer's copy",
    async run(t) {
      const a = t.dir("clip-x/a");
      const b = t.dir("clip-x/b");
      const c = t.dir("clip-x/c");
      t.mk(path.join(a, "both.txt"), "x");
      await t.nav(a);
      await t.select(["both.txt"]);
      const into = (dir) => path.join(dir, "both.txt");
      for (let i = 0; i < 4 && !t.exists(into(b)); i++) {
        await t.call("copySel", false);
        // The shell's own Paste verb on folder b: what Explorer's Paste runs.
        await t.psCmd(
          `(New-Object -ComObject Shell.Application).Namespace('${b}').Self.InvokeVerb('Paste'); ` +
            `for ($n = 0; $n -lt 50 -and -not (Test-Path -LiteralPath '${into(b)}'); $n++) { Start-Sleep -Milliseconds 100 }`,
        );
      }
      t.ok(t.exists(into(b)), "Explorer pasted Filezy's copy");
      await t.nav(c);
      for (let i = 0; i < 4 && !t.exists(into(c)); i++) {
        // The shell's Copy verb, kept alive until Filezy has pasted (it owns the clipboard data).
        const copying = t.psCmd(
          `(New-Object -ComObject Shell.Application).Namespace('${b}').ParseName('both.txt').InvokeVerb('copy'); ` +
            `for ($n = 0; $n -lt 80 -and -not (Test-Path -LiteralPath '${into(c)}'); $n++) { Start-Sleep -Milliseconds 100 }`,
        );
        await t.waitFor("__e2e.app.canPaste", 5000, "the copy reached Filezy").catch(() => {});
        await t.call("paste");
        await settleOp(t);
        await copying;
      }
      t.ok(t.exists(into(c)), "Filezy pasted Explorer's copy");
    },
  },
];
