import fs from "node:fs";
import path from "node:path";

const active = (t) =>
  t.ev(`(() => { const a = document.activeElement; return a ? (a.className || a.tagName).toString() : ""; })()`);
const focusList = async (t) => {
  for (let i = 0; i < 4 && !(await active(t)).includes("scroller"); i++) await t.key("F6");
  t.ok((await active(t)).includes("scroller"), `F6 should reach the file list, focus is on ${await active(t)}`);
};
const selected = (t) => t.selectedNames();

export default [
  {
    name: "Ctrl+Shift+N creates a folder instead of opening a window",
    async run(t) {
      const d = t.dir("kb-new-folder");
      t.mk(path.join(d, "a.txt"), "a");
      await t.nav(d);
      await t.key("N", { ctrl: true, shift: true });
      await t.waitFor("__e2e.app.renaming >= 0", 5000, "no rename box after Ctrl+Shift+N");
      t.ok(t.exists(path.join(d, "New folder")), "New folder was not created on disk");
      await t.key("Escape");
    },
  },
  {
    name: "browse, create, rename, copy and paste using only the keyboard",
    async run(t) {
      const d = t.dir("kb-only");
      for (const n of ["apple.txt", "banana.txt", "cherry.txt"]) t.mk(path.join(d, n), n);
      t.dir(path.join("kb-only", "sub"));
      await t.nav(d);
      await focusList(t);
      await t.key("ArrowDown"); // the first press lands on the first item
      t.eq(await selected(t), ["sub"], "first item");
      await t.key("Enter");
      await t.settle();
      t.eq(await t.ev("__e2e.app.path"), path.join(d, "sub"), "Enter opened the folder");
      await t.key("Backspace");
      await t.settle();
      t.eq(await selected(t), ["sub"], "Backspace came back with the folder selected");

      await t.key("N", { ctrl: true, shift: true });
      await t.waitFor("!!document.querySelector('input.rename')", 5000, "rename box");
      await t.key("a", { ctrl: true });
      await t.type("kbdir");
      await t.key("Enter");
      await t.until(() => t.exists(path.join(d, "kbdir")), 5000, "typed folder name");
      await t.settle();

      await t.type("b");
      t.eq(await selected(t), ["banana.txt"], "type-to-select");
      await t.key("ArrowDown", { shift: true });
      t.eq(await selected(t), ["banana.txt", "cherry.txt"], "Shift+Down extends");
      await t.key("c", { ctrl: true });
      await t.until(async () => (await t.clipboard()).files.length === 2, 5000, "copied to the clipboard");
      await t.sleep(1100); // let the type-ahead buffer expire
      await t.type("k");
      t.eq(await selected(t), ["kbdir"], "type-to-select a folder");
      await t.key("Enter");
      await t.settle();
      await t.key("v", { ctrl: true });
      await t.until(() => t.exists(path.join(d, "kbdir", "banana.txt")) && t.exists(path.join(d, "kbdir", "cherry.txt")), 10000, "pasted");

      await t.settle();
      t.eq(await selected(t), ["banana.txt", "cherry.txt"], "the pasted items are selected");
      await t.key("Home");
      await t.key("F2");
      await t.waitFor("!!document.querySelector('input.rename')", 5000, "F2 rename box");
      await t.key("Escape");
    },
  },
  {
    name: "search, the context menu and tabs from the keyboard",
    async run(t) {
      const d = t.dir("kb-more");
      for (const n of ["one.txt", "two.txt"]) t.mk(path.join(d, n));
      t.mk(path.join(d, "deep", "two-deep.txt"));
      await t.nav(d);
      await focusList(t);
      await t.key("f", { ctrl: true });
      t.ok((await active(t)) === "" || (await t.ev("document.activeElement?.getAttribute('aria-label')")) === "Search", "Ctrl+F focused the search box");
      await t.type("two");
      await t.waitFor("__e2e.app.search?.done", 10000, "search ran");
      await t.settle();
      t.eq((await t.names()).sort(), ["two-deep.txt", "two.txt"], "results");
      await t.key("Escape");
      await t.settle();
      t.eq(await t.ev("__e2e.app.search"), null, "Escape cleared the search");

      await focusList(t);
      await t.key("ArrowDown");
      await t.key("F10", { shift: true });
      await t.waitFor("!!document.querySelector('.ctxmenu')", 3000, "Shift+F10 opened the menu");
      t.ok(await t.ev("document.activeElement?.classList.contains('menu-item') || !!document.activeElement?.closest('.ctxmenu')"), "focus moved into the menu");
      const before = await selected(t);
      await t.key("ArrowDown");
      await t.key("ArrowDown");
      t.eq(await selected(t), before, "arrows in the menu leave the file selection alone");
      await t.key("Escape");
      await t.waitFor("!document.querySelector('.ctxmenu')", 3000, "Escape closed it");
      t.ok((await active(t)).includes("scroller"), "focus returned to the list");

      const first = await t.ev("__e2e.app.activeTab");
      await t.key("t", { ctrl: true });
      await t.waitFor(`__e2e.tabs.length === 2 && __e2e.app.activeTab !== ${first}`, 5000, "Ctrl+T opened a tab");
      await t.settle();
      await t.key("Tab", { ctrl: true });
      await t.settle();
      t.eq(await t.ev("__e2e.app.activeTab"), first, "Ctrl+Tab came back");
      await t.key("1", { ctrl: true });
      t.eq(await t.ev("__e2e.app.activeTab"), first, "Ctrl+1 is the first tab");
    },
  },
  {
    name: "the tree takes arrows without touching the file selection",
    async run(t) {
      const d = t.dir("kb-tree");
      t.mk(path.join(d, "x.txt"));
      await t.nav(d);
      await t.select(["x.txt"]);
      await t.key("F6"); // address bar
      await t.key("F6"); // tree
      t.ok(await t.ev("!!document.activeElement?.closest('.navpane')"), "F6 reached the tree");
      const label = () => t.ev("document.activeElement?.querySelector('.label')?.textContent");
      await t.key("Home");
      t.eq(await label(), "Home", "Home key");
      await t.key("ArrowDown");
      await t.key("ArrowDown");
      t.eq(await selected(t), ["x.txt"], "file selection untouched");
      await t.key("End");
      t.eq(await label(), "Recycle Bin", "End key");
      await t.key("Enter");
      await t.waitFor("__e2e.app.path === '::RecycleBin'", 3000, "Enter opened the bin");
    },
  },
  {
    name: "restore a deleted file using only the keyboard",
    async run(t) {
      const d = t.dir("kb-restore");
      const name = `kb-restore-${t.token}.txt`;
      const f = t.mk(path.join(d, name), "r");
      await t.nav(d);
      await focusList(t);
      await t.key("Home");
      await t.key("Delete");
      await t.until(() => !t.exists(f), 5000, "deleted");
      await t.settle();
      // To the bin through the tree, then into its list.
      await t.key("F6");
      await t.key("F6");
      await t.key("End");
      await t.key("Enter");
      await t.waitFor(`[...document.querySelectorAll(".rrow")].some((r) => r.textContent.includes(${JSON.stringify(name)}))`, 10000, "listed");
      await t.ev(`document.querySelector(".rows").focus()`);
      // Walk the list until the keyboard is on our item.
      for (let i = 0; i < 2000; i++) {
        await t.key(i === 0 ? "Home" : "ArrowDown");
        if (await t.ev(`!!document.querySelector(".rrow.focused")?.textContent.includes(${JSON.stringify(name)})`)) break;
      }
      await t.key("Enter");
      await t.until(() => t.exists(f), 10000, "Enter restored it");
    },
  },
  {
    name: "type-to-select, paging and ranges follow Explorer",
    async run(t) {
      const d = t.dir("kb-nav");
      for (const n of ["alpha1", "alpha2", "beta", "betamax", "gamma"]) t.mk(path.join(d, `${n}.txt`));
      for (let i = 0; i < 60; i++) t.mk(path.join(d, `z${String(i).padStart(2, "0")}.txt`));
      await t.nav(d);
      await focusList(t);
      await t.type("a");
      t.eq(await selected(t), ["alpha1.txt"], "a");
      await t.type("a");
      t.eq(await selected(t), ["alpha2.txt"], "a again steps on");
      await t.sleep(1100);
      await t.type("betam");
      t.eq(await selected(t), ["betamax.txt"], "a longer prefix");
      await t.key("Home");
      await t.key("PageDown", { shift: true });
      const n = (await selected(t)).length;
      t.ok(n > 3 && n < 65, `Shift+PageDown selects a page (${n})`);
      await t.key("End", { shift: true });
      t.ok(await t.ev("__e2e.app.allSelected || __e2e.sel.size === __e2e.app.total"), "Shift+End from the top selects all");
      await t.key("Home");
      await t.key("ArrowDown", { ctrl: true });
      await t.key("ArrowDown", { ctrl: true });
      await t.key(" ", { ctrl: true });
      t.eq(await selected(t), ["alpha1.txt", "beta.txt"], "Ctrl+arrows move without selecting; Ctrl+Space adds");
    },
  },
  {
    name: "Select none, Invert selection and item check boxes",
    async run(t) {
      const d = t.dir("kb-invert");
      for (const n of ["a.txt", "b.txt", "c.txt"]) t.mk(path.join(d, n));
      await t.nav(d);
      await t.select(["b.txt"]);
      await t.call("invertSelection");
      t.eq(await selected(t), ["a.txt", "c.txt"], "inverted");
      await t.call("selectNone");
      t.eq(await selected(t), [], "none");
      await t.call("setSetting", "checkboxes", true);
      try {
        await t.waitFor("document.querySelectorAll('.vrow .ck').length === 3", 3000, "check boxes shown");
        const box = await t.ev(`(() => {
          const r = [...document.querySelectorAll(".vrow")].find((r) => r.textContent.includes("c.txt"));
          const b = r.querySelector(".ck").getBoundingClientRect();
          return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
        })()`);
        await t.select(["a.txt"]);
        await t.click(box);
        t.eq(await selected(t), ["a.txt", "c.txt"], "a check box adds to the selection");
      } finally {
        await t.call("setSetting", "checkboxes", false);
      }
    },
  },
  {
    name: "screen readers get a named, multiselect list with positions",
    async run(t) {
      const d = t.dir("kb-a11y");
      for (const n of ["first.txt", "second.txt"]) t.mk(path.join(d, n));
      await t.nav(d);
      await t.select(["second.txt"]);
      await t.send("Accessibility.enable");
      const { nodes } = await t.send("Accessibility.getFullAXTree");
      const prop = (n, name) => n.properties?.find((p) => p.name === name)?.value?.value;
      const list = nodes.find((n) => n.role?.value === "listbox" && n.name?.value === "kb-a11y");
      t.ok(list, "listbox named after the folder");
      t.eq(prop(list, "multiselectable"), true, "multiselectable");
      const opts = nodes.filter((n) => n.role?.value === "option" && /^(first|second)\.txt/.test(n.name?.value ?? ""));
      t.eq(opts.length, 2, "both rows are options");
      const second = opts.find((n) => n.name.value.startsWith("second"));
      t.eq(prop(second, "selected"), true, "selection exposed");
      // DevTools doesn't report set size or position; Chromium passes these
      // attributes to UI Automation as SizeOfSet and PositionInSet.
      t.eq(await t.ev(`(() => { const r = [...document.querySelectorAll("[role=option]")].find((r) => r.textContent.includes("second.txt")); return [r.getAttribute("aria-setsize"), r.getAttribute("aria-posinset")]; })()`), ["2", "2"], "set size and position");
      const switches = nodes.filter((n) => n.role?.value === "switch");
      t.ok(switches.every((n) => n.name?.value), "every switch on screen has a name");
      // An icon glyph alone (a private-use character) is not a name.
      const unnamed = nodes.filter((n) => n.role?.value === "button" && !n.ignored && !/[a-z]/i.test(n.name?.value ?? ""));
      t.eq(unnamed.map((n) => n.name?.value ?? ""), [], "buttons without a readable name");
    },
  },
];
