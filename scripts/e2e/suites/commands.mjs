import fs from "node:fs";
import path from "node:path";

export default [
  {
    name: "batch rename numbers items, keeps extensions, skips taken names, and undoes in one step",
    async run(t) {
      const d = t.dir("batch-rename");
      t.mk(path.join(d, "a.txt"), "a");
      t.mk(path.join(d, "b.md"), "b");
      t.dir(path.join("batch-rename", "sub"));
      t.mk(path.join(d, "Report (3).md"), "already here");
      await t.nav(d);
      await t.select(["sub", "a.txt", "b.md"]);
      await t.key("F2");
      await t.waitFor("__e2e.batch.open", 3000, "batch dialog");
      await t.waitFor("!!document.querySelector('.dlg input')", 3000, "name field");
      await t.ev(`(() => { const i = document.querySelector(".dlg input"); i.value = "Report"; i.dispatchEvent(new Event("input", { bubbles: true })); })()`);
      await t.waitFor(`[...document.querySelectorAll(".prow")].map((r) => r.querySelector(".new").textContent).join("|") === "Report (1)|Report (2).txt|Report (3).md"`, 3000, "preview names");
      await t.waitFor(`document.querySelectorAll(".prow.bad").length === 1`, 3000, "the taken name is flagged");
      await t.key("Enter");
      await t.settle();
      const files = fs.readdirSync(d).sort();
      t.eq(files, ["Report (1)", "Report (2).txt", "Report (3).md", "b.md"], "renamed on disk; the clash kept its name");
      t.eq(t.read(path.join(d, "Report (3).md")), "already here", "the existing file is untouched");
      t.ok((await t.ev("__e2e.app.status")).includes("Renamed 2 of 3"), await t.ev("__e2e.app.status"));
      await t.key("z", { ctrl: true });
      await t.settle();
      t.eq(fs.readdirSync(d).sort(), ["Report (3).md", "a.txt", "b.md", "sub"], "one undo restores both");
    },
  },
  {
    name: "changing a file's extension asks first, and No keeps the name",
    async run(t) {
      const d = t.dir("ext-change");
      t.mk(path.join(d, "report.txt"), "r");
      await t.nav(d);
      const rename = async (to) => {
        await t.select([fs.readdirSync(d)[0]]);
        return t.ev(`__e2e.commitRename(${await t.ev("[...__e2e.sel][0]")}, ${JSON.stringify(to)})`);
      };
      const no = rename("report.md");
      t.ok((await t.answer(false)).includes("might become unusable"), "Explorer's warning");
      await no;
      await t.settle();
      t.eq(fs.readdirSync(d), ["report.txt"], "No keeps the old name");
      const yes = rename("report.md");
      await t.answer(true);
      await yes;
      await t.settle();
      t.eq(fs.readdirSync(d), ["report.md"], "Yes renames");
    },
  },
  {
    name: "opening more than 15 items asks first",
    async run(t) {
      const d = t.dir("open-many");
      // Folders, so Yes opens tabs here instead of launching apps on the desktop.
      const names = Array.from({ length: 16 }, (_, i) => `f${String(i).padStart(2, "0")}`);
      for (const n of names) t.dir(path.join("open-many", n));
      await t.nav(d);
      await t.select(names);
      const no = t.call("openSelection");
      t.ok((await t.answer(false)).includes("open 16 items"), "asks with the count");
      await no;
      t.eq(await t.ev("__e2e.tabs.length"), 1, "No opens nothing");
      const yes = t.call("openSelection");
      await t.answer(true);
      await yes;
      await t.waitFor("__e2e.tabs.length === 17", 10000, "Yes opens each folder in a tab");
    },
  },
  {
    name: "Create shortcut and Paste shortcut make Explorer-named links",
    async run(t) {
      const d = t.dir("shortcuts");
      const other = t.dir("shortcuts-dest");
      t.mk(path.join(d, "doc.txt"), "d");
      await t.nav(d);
      await t.select(["doc.txt"]);
      await t.call("createShortcuts");
      await t.settle();
      t.ok(t.exists(path.join(d, "doc.txt - Shortcut.lnk")), "shortcut next to the item");
      t.eq(await t.selectedNames(), ["doc.txt - Shortcut.lnk"], "the new shortcut is selected");
      await t.select(["doc.txt"]);
      await t.copyToClipboard();
      await t.nav(other);
      await t.call("pasteShortcuts");
      await t.settle();
      t.ok(t.exists(path.join(other, "doc.txt - Shortcut.lnk")), "pasted shortcut");
    },
  },
  {
    name: "Properties opens one sheet for a multi-selection",
    async run(t) {
      const d = t.dir("multi-props");
      for (const n of ["p1.txt", "p2.txt"]) t.mk(path.join(d, n), n);
      await t.nav(d);
      await t.select(["p1.txt", "p2.txt"]);
      await t.key("Enter", { alt: true });
      const found = await t.uia("-Window", "*Properties*", "-Timeout", "10");
      // Windows titles the combined sheet "first, ... Properties".
      t.ok(/p1\.txt, \.\.\. Properties/.test(found), `a combined sheet: ${found}`);
      await t.uia("-Window", "*Properties*", "-Close");
    },
  },
  {
    name: "Share opens Windows' share sheet for files and stays off for folders",
    async run(t) {
      const d = t.dir("share");
      for (const n of ["one.txt", "two.txt"]) t.mk(path.join(d, n), n);
      t.dir(path.join("share", "folder"));
      await t.nav(d);
      const shareOff = `document.querySelector('.cbtn[aria-label="Share"]').disabled`;
      const menuShare = `[...document.querySelectorAll(".ctxmenu .menu-item")].some((b) => b.lastChild?.textContent.trim() === "Share")`;
      await t.select(["folder", "one.txt"]);
      await t.waitFor(shareOff, 3000, "Share is off when a folder is selected");
      await t.key("F10", { shift: true });
      await t.waitFor("!!document.querySelector('.ctxmenu')", 3000, "item menu");
      await t.sleep(300);
      t.ok(!(await t.ev(menuShare)), "the item menu leaves Share out too");
      await t.key("Escape");
      await t.select(["one.txt", "two.txt"]);
      await t.waitFor(`!${shareOff}`, 3000, "Share is on for files");
      await t.key("F10", { shift: true });
      await t.waitFor(menuShare, 3000, "Share in the item menu");
      await t.key("Escape");
      await t.click(".cbtn", { where: { title: "Share" } });
      try {
        t.eq(await t.ps("share.ps1"), "You are sharing 2 files", "the sheet shares both files");
      } finally {
        await t.ps("share.ps1", "-Close").catch(() => {});
      }
    },
  },
  {
    name: "Open in new tab opens every selected folder",
    async run(t) {
      const d = t.dir("new-tabs");
      for (const n of ["one", "two"]) t.dir(path.join("new-tabs", n));
      await t.nav(d);
      await t.select(["one", "two"]);
      await t.call("openInNewTabs");
      await t.waitFor("__e2e.tabs.length === 3", 5000, "two new tabs");
      await t.settle();
      const paths = await t.ev("__e2e.tabs.map((x) => x.path)");
      t.eq(paths.slice(1), [path.join(d, "one"), path.join(d, "two")], "tab folders");
    },
  },
  {
    name: "Open file location from a search result selects it in its folder",
    async run(t) {
      const d = t.dir("open-location");
      const f = t.mk(path.join(d, "deep", "er", "target-file.txt"));
      await t.nav(d);
      await t.call("startSearch", "target-file");
      await t.waitFor("__e2e.app.search?.done", 10000, "search");
      await t.settle();
      await t.call("openFileLocation", f);
      await t.settle();
      t.eq(await t.ev("__e2e.app.path"), path.dirname(f), "the file's folder");
      t.eq(await t.selectedNames(), ["target-file.txt"], "selected there");
    },
  },
  {
    name: "launching with /select,<file> opens its folder with the file selected",
    async run(t) {
      const d = t.dir("launch select");
      t.mk(path.join(d, "other.txt"));
      const f = t.mk(path.join(d, "chosen file ü.txt"));
      await t.restart([`/select,${f}`]);
      try {
        await t.settle();
        t.eq(await t.ev("__e2e.app.path"), d, "folder");
        t.eq(await t.selectedNames(), ["chosen file ü.txt"], "selection");
      } finally {
        await t.restart();
      }
    },
  },
  {
    name: "Open in new window opens that folder in a second window",
    async run(t) {
      const d = t.dir("new-window");
      t.dir(path.join("new-window", "inner"));
      t.mk(path.join(d, "inner", "x.txt"));
      await t.nav(d);
      await t.select(["inner"]);
      await t.call("openInNewWindow");
      const w = await t.otherWindow();
      try {
        t.eq(await w.ev("__e2e.app.path"), path.join(d, "inner"), "second window's folder");
        await w.ev("void __e2e.closeTab(__e2e.app.activeTab)").catch(() => {}); // closes that window
      } finally {
        w.close();
      }
    },
  },
  {
    name: "New > Text Document makes an empty file and opens its rename box",
    async run(t) {
      const d = t.dir("new-text");
      t.mk(path.join(d, "a.txt"), "a");
      await t.nav(d);
      await t.call("loadNewItems");
      await t.waitFor("!!__e2e.newItems.list", 5000, "the New menu's types");
      t.ok(await t.ev("__e2e.newItems.list.some((i) => i.ext === 'txt')"), "Text Document is offered");
      t.ok(await t.ev("!__e2e.newItems.list.some((i) => i.ext === 'lnk' || i.ext === 'library-ms')"), "no handler-only types");
      await t.call("newFile", "txt");
      await t.waitFor("__e2e.app.renaming >= 0", 5000, "rename box");
      const made = fs.readdirSync(d).find((f) => f !== "a.txt");
      t.ok(made && made.endsWith(".txt"), `made ${made}`);
      t.eq(fs.statSync(path.join(d, made)).size, 0, "empty");
      // The row arrives with the next fetch after the re-list.
      await t.waitFor(`__e2e.rowAt(__e2e.app.renaming)?.name === ${JSON.stringify(made)}`, 5000, "the box is on the new file");
      await t.ev("__e2e.app.renaming = -1");
    },
  },
  {
    name: "a saved workspace can't be renamed to a name another one has",
    async run(t) {
      await t.ev(`(() => { __e2e.saved.workspaces.length = 0; __e2e.saved.workspaces.push({ name: "Work", tabs: ["C:\\\\"] }, { name: "Play", tabs: ["C:\\\\"] }); })()`);
      const done = t.ev("__e2e.renameSaved('workspaces', 1)");
      await t.waitFor("__e2e.dialog.ask?.value === 'Play'", 3000, "the rename prompt");
      await t.ev(`(() => { const i = document.querySelector("dialog input"); i.value = "work"; i.dispatchEvent(new Event("input", { bubbles: true })); document.querySelector("dialog .btn.primary").click(); })()`);
      await done;
      t.eq(await t.ev("__e2e.saved.workspaces.map((w) => w.name)"), ["Work", "Play"], "both keep their names");
      t.ok((await t.ev("__e2e.app.status")).includes("already one named"), "says why");
      await t.ev("__e2e.saved.workspaces.length = 0");
    },
  },
  {
    name: "switching Copy off in Settings takes it off the item menu",
    async run(t) {
      const d = t.dir("menu-switch");
      t.mk(path.join(d, "a.txt"), "a");
      await t.nav(d);
      await t.select(["a.txt"]);
      const copyShown = async () => {
        await t.ev("__e2e.hooks.contextMenu()");
        await t.waitFor("!!document.querySelector('.ctxmenu')", 3000, "item menu");
        const shown = await t.ev(`!!document.querySelector('.ctxmenu button[aria-label="Copy"]')`);
        await t.ev("document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))");
        await t.ev("document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))");
        await t.waitFor("!document.querySelector('.ctxmenu')", 3000, "menu closed");
        return shown;
      };
      t.ok(await copyShown(), "Copy is on by default");
      await t.call("setMenuOn", "file.copy", false);
      try {
        t.ok(!(await copyShown()), "switched off, it's gone");
      } finally {
        await t.call("setMenuOn", "file.copy", true);
      }
    },
  },
];
