// The shell's context menus: built for the whole selection, filled submenus,
// commands tied to the menu they came from, and Windows' own menu.
import fs from "node:fs";
import path from "node:path";

/** Open the item menu for the selection and wait for the shell's entries. */
async function itemMenu(t) {
  await t.ev("__e2e.app.focus < 0 && (__e2e.app.focus = [...__e2e.sel][0] ?? 0)");
  await t.ev("__e2e.hooks.contextMenu()");
  await t.waitFor("!!document.querySelector('.ctxmenu') && !document.querySelector('.ctxmenu .menuload')", 60000, "the shell's entries");
}

/** The shell entries' texts at one level of the open menu (a submenu, given the path to it). */
const entries = (t, to = []) =>
  t.ev(`(async () => {
    let scope = document.querySelector(".ctxmenu");
    for (const name of ${JSON.stringify(to)}) {
      const w = [...scope.querySelectorAll(".subwrap")].find((x) => x.closest(".menu") === scope && x.querySelector(".mtext")?.textContent === name);
      if (!w) return null;
      w.dispatchEvent(new MouseEvent("mouseenter"));
      await new Promise((r) => setTimeout(r, 150));
      scope = w.querySelector(".submenu");
    }
    return [...scope.querySelectorAll(".subwrap")].filter((x) => x.closest(".menu") === scope).map((x) => x.querySelector(".mtext").textContent);
  })()`);

/** Click a shell entry by its path of texts, e.g. ["Send to", "Compressed (zipped) folder"]. */
const pick = (t, names) =>
  t.ev(`(async () => {
    const names = ${JSON.stringify(names)};
    let scope = document.querySelector(".ctxmenu");
    for (let i = 0; i < names.length; i++) {
      const w = [...scope.querySelectorAll(".subwrap")].find((x) => x.closest(".menu") === scope && x.querySelector(".mtext")?.textContent === names[i]);
      if (!w) throw new Error("no menu entry " + names[i]);
      if (i === names.length - 1) return w.querySelector("button").click();
      w.dispatchEvent(new MouseEvent("mouseenter"));
      await new Promise((r) => setTimeout(r, 150));
      scope = w.querySelector(".submenu");
    }
  })()`);

/** The id of a shell entry in a backend menu, by its path of texts. */
function idOf(items, names) {
  let list = items;
  let it;
  for (const n of names) {
    it = list.find((x) => x.text === n);
    if (!it) return null;
    list = it.sub ?? [];
  }
  return it.id;
}

const zipIn = (dir) => fs.readdirSync(dir).filter((n) => n.endsWith(".zip"));

export default [
  {
    name: "Send to > Compressed (zipped) folder takes every selected item, well past 100",
    async run(t) {
      const d = t.dir("sm-many");
      for (let i = 0; i < 150; i++) t.mk(path.join(d, `f${String(i).padStart(3, "0")}.txt`), String(i));
      await t.nav(d);
      await t.call("selectAll");
      await itemMenu(t);
      await pick(t, ["Send to", "Compressed (zipped) folder"]);
      await t.until(() => zipIn(d).length === 1, 30000, "a zip appears");
      t.eq((await t.zipEntries(path.join(d, zipIn(d)[0]))).length, 150, "all 150 items are in it");
    },
  },
  {
    name: "past the item limit, the menu says why there are no shell entries instead of using some of them",
    async run(t) {
      const d = t.dir("sm-over");
      // The runner caps menus at 200 items (FILEZY_E2E_MENU_MAX); the app's own cap is 10,000.
      for (let i = 0; i < 201; i++) t.mk(path.join(d, `f${String(i).padStart(3, "0")}.txt`), "");
      await t.nav(d);
      await t.call("selectAll");
      await itemMenu(t);
      t.eq(await t.ev("document.querySelector('.ctxmenu [role=note]')?.textContent"), "Shell options aren't shown for more than 200 items.", "the note");
      t.eq(await entries(t), [], "no partial list of entries");
      t.ok(await t.ev(`[...document.querySelectorAll(".ctxmenu .menu-item")].some((b) => b.textContent.includes("Properties"))`), "Filezy's own entries are all there");
    },
  },
  {
    name: "a drive tile's menu has the shell's entries for the drive",
    async run(t) {
      await t.nav("::ThisPC");
      const drive = path.parse(t.root).root;
      await t.click(".drivetile", { button: "right", where: { title: drive } });
      await t.waitFor("!!document.querySelector('.drivemenu') && !document.querySelector('.drivemenu .loading')", 60000, "the drive's shell entries");
      const texts = await t.ev(`[...document.querySelectorAll(".drivemenu .mtext")].map((e) => e.textContent)`);
      t.ok(texts.length > 0, `shell entries: ${texts.join(", ")}`);
      t.ok(await t.ev(`[...document.querySelectorAll(".drivemenu .menu-item")].some((b) => b.textContent.includes("Show more options"))`), "and Show more options");
      await t.key("Escape");
    },
  },
  {
    name: "a search selection across folders gets one shell menu for all of it",
    async run(t) {
      const root = t.dir("sm-search");
      t.mk(path.join(root, "a", "xs-one.txt"), "1");
      t.mk(path.join(root, "b", "xs-two.txt"), "2");
      // The first menu in a fresh process: Send to fills in its targets for
      // items in several folders only after the menu is built.
      await t.restart();
      await t.nav(root);
      await t.call("startSearch", "xs-");
      await t.waitFor("__e2e.app.search?.done", 10000, "search");
      await t.settle();
      await t.call("selectAll");
      await itemMenu(t);
      for (let i = 0; !((await entries(t, ["Send to"])) ?? []).includes("Compressed (zipped) folder"); i++) {
        if (i > 50) throw new Error(`Send to never listed Compressed (zipped) folder: ${JSON.stringify(await entries(t, ["Send to"]))}`);
        await t.sleep(100);
      }
      await pick(t, ["Send to", "Compressed (zipped) folder"]);
      const zips = () => [...zipIn(path.join(root, "a")).map((n) => path.join(root, "a", n)), ...zipIn(path.join(root, "b")).map((n) => path.join(root, "b", n))];
      await t.until(() => zips().length === 1, 30000, "a zip appears");
      t.eq((await t.zipEntries(zips()[0])).length, 2, "both results are in it");
    },
  },
  {
    name: "Open with and Send to come filled in, and so do archive tools' submenus",
    async run(t) {
      const d = t.dir("sm-subs");
      t.mk(path.join(d, "note.txt"), "n");
      await t.nav(d);
      await t.select(["note.txt"]);
      await itemMenu(t);
      t.ok(((await entries(t, ["Open with"])) ?? []).length > 0, "Open with lists apps");
      t.ok(((await entries(t, ["Send to"])) ?? []).includes("Compressed (zipped) folder"), "Send to lists its targets");
      const top = await entries(t);
      for (const tool of ["7-Zip", "WinRAR"].filter((x) => top.includes(x))) {
        t.ok(((await entries(t, [tool])) ?? []).length > 0, `${tool}'s submenu has entries`);
      }
    },
  },
  {
    name: "a command from an older menu is refused instead of running on the new selection",
    async run(t) {
      const x = t.dir("sm-stale/x");
      const y = t.dir("sm-stale/y");
      t.mk(path.join(x, "a.txt"), "a");
      t.mk(path.join(y, "b.txt"), "b");
      const zip = ["Send to", "Compressed (zipped) folder"];
      await t.nav(x);
      await t.select(["a.txt"]);
      const a = await t.ev(`__e2e.invoke("shell_menu", { sel: __e2e.selRef() })`);
      await t.nav(y);
      await t.select(["b.txt"]);
      const b = await t.ev(`__e2e.invoke("shell_menu", { sel: __e2e.selRef() })`);
      t.ok(a.session !== b.session, "each menu has its own session");
      const err = await t.ev(`__e2e.invoke("shell_menu_invoke", { session: ${a.session}, id: ${idOf(a.items, zip)} }).then(() => "ran", (e) => String(e))`);
      t.ok(err.includes("out of date"), err);
      await t.sleep(2000);
      t.eq([zipIn(x), zipIn(y)], [[], []], "nothing was zipped");
      await t.ev(`__e2e.invoke("shell_menu_invoke", { session: ${b.session}, id: ${idOf(b.items, zip)} })`);
      await t.until(() => zipIn(y).length === 1, 30000, "the current menu's command runs");
    },
  },
  {
    name: "Show more options opens Windows' own menu, whose built-in commands run Filezy's versions",
    async run(t) {
      const d = t.dir("sm-native");
      // Named with the run's token: should a wrong entry ever delete it, the
      // runner's Recycle Bin cleanup puts it back.
      const name = `doc-${t.token}.txt`;
      t.mk(path.join(d, name), "d");
      await t.nav(d);
      await t.select([name]);
      // A user's click brings the window to the front; Windows' menu needs that.
      await t.foreground();
      await itemMenu(t);
      await t.ev(`[...document.querySelectorAll(".ctxmenu .menu-item")].find((b) => b.textContent.includes("Show more options")).click()`);
      const n = Number(await t.uia("-Menu", "-Count"));
      t.ok(n > 10, `the full menu, ${n} entries`);
      await t.uia("-Menu", "-Key", "27");
      await t.until(async () => (await t.uia("-Menu", "-Count", "-Timeout", "1").catch(() => "")) === "", 5000, "Escape closed it");
      await itemMenu(t);
      await t.ev(`[...document.querySelectorAll(".ctxmenu .menu-item")].find((b) => b.textContent.includes("Show more options")).click()`);
      await t.uia("-Menu", "-Choose", "Rename");
      await t.waitFor("__e2e.app.renaming >= 0", 5000, "Filezy's own rename box opened");
      await t.key("Escape");
    },
  },
];
