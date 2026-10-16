import path from "node:path";

export default [
  {
    name: "a refresh of the old folder cannot replace a newer navigation",
    async run(t) {
      const x = t.dir("nav-race/X");
      const y = t.dir("nav-race/Y");
      t.mk(path.join(x, "x1.txt"));
      t.mk(path.join(y, "y1.txt"));
      t.mk(path.join(y, "y2.txt"));
      await t.nav(x);
      // Start navigating to Y, then refresh while that navigation is in flight:
      // what a watcher event for X does when it fires during the navigation.
      await t.ev(`(() => { __e2e.navigate(${JSON.stringify(y)}); __e2e.refresh(); })()`);
      await t.sleep(400);
      await t.settle();
      t.eq(await t.ev("__e2e.app.path"), y, "address");
      t.eq(await t.names(), ["y1.txt", "y2.txt"], "listing");
      t.eq(await t.ev("__e2e.app.total"), 2, "item count");
    },
  },
  {
    name: "a slow navigation finishing after a tab switch stays in its own tab",
    async run(t) {
      const a = t.dir("nav-tabs/A");
      const b = t.dir("nav-tabs/B");
      t.mk(path.join(a, "a.txt"));
      t.mk(path.join(b, "b1.txt"));
      t.mk(path.join(b, "b2.txt"));
      await t.nav(b);
      const first = await t.ev("__e2e.app.activeTab");
      await t.call("newTab", b);
      await t.settle();
      const second = await t.ev("__e2e.app.activeTab");
      // Navigate tab 2 to A and switch back to tab 1 before it can finish.
      await t.ev(`(() => { __e2e.navigate(${JSON.stringify(a)}); __e2e.switchTab(${first}); })()`);
      await t.sleep(500);
      await t.settle();
      t.eq(await t.ev("__e2e.app.activeTab"), first, "active tab");
      t.eq(await t.ev("__e2e.app.path"), b, "tab 1 address");
      t.eq(await t.names(), ["b1.txt", "b2.txt"], "tab 1 listing");
      t.ok(
        !(await t.ev(`__e2e.tabs.find((x) => x.id === ${first}).hist.includes(${JSON.stringify(a)})`)),
        "tab 1 history picked up tab 2's navigation",
      );
      t.eq(
        await t.ev(`__e2e.tabs.find((x) => x.id === ${second}).path`),
        a,
        "tab 2 keeps the folder it navigated to",
      );
    },
  },
  {
    name: "an external change keeps the selection and an open rename with its typed text",
    async run(t) {
      const d = t.dir("refresh-keep");
      for (const n of ["a.txt", "b.txt", "c.txt"]) t.mk(path.join(d, n));
      await t.nav(d);
      await t.select(["b.txt"]);
      await t.call("startRename");
      await t.waitFor("!!document.querySelector('input.rename')", 3000, "rename box");
      await t.ev(`(() => {
        const i = document.querySelector("input.rename");
        i.value = "b-renamed";
        i.dispatchEvent(new Event("input", { bubbles: true }));
      })()`);
      // A file that sorts before b.txt shifts b's row index.
      t.mk(path.join(d, "a0.txt"));
      await t.waitFor("__e2e.app.total === 4", 5000, "watcher refresh");
      await t.sleep(300);
      t.eq(await t.selectedNames(), ["b.txt"], "selection");
      t.ok(await t.ev("__e2e.rows[__e2e.app.renaming]?.name === 'b.txt'"), "rename still on b.txt");
      t.eq(await t.ev("document.querySelector('input.rename')?.value"), "b-renamed", "typed text");
      await t.key("Escape");
      t.ok(t.exists(path.join(d, "b.txt")), "Escape keeps the old name");
    },
  },
  {
    name: "Up and Back select the folder you came from",
    async run(t) {
      const d = t.dir("up-select");
      for (const n of ["alpha", "beta", "gamma"]) t.dir(path.join("up-select", n));
      await t.nav(path.join(d, "beta"));
      await t.call("up");
      await t.settle();
      t.eq(await t.ev("__e2e.app.path"), d, "up went to the parent");
      t.eq(await t.selectedNames(), ["beta"], "after Up");
      await t.nav(path.join(d, "gamma"));
      await t.call("back");
      await t.settle();
      t.eq(await t.selectedNames(), ["gamma"], "after Back");
    },
  },
  {
    name: "a tab gets its selection and search back when reselected",
    async run(t) {
      const d = t.dir("tab-keep");
      for (const n of ["a.txt", "b.txt", "c.txt"]) t.mk(path.join(d, n));
      t.mk(path.join(d, "sub", "deep-match.txt"));
      await t.nav(d);
      await t.select(["b.txt", "c.txt"]);
      const first = await t.ev("__e2e.app.activeTab");
      await t.call("newTab", path.join(d, "sub"));
      await t.settle();
      await t.call("switchTab", first);
      await t.settle();
      t.eq(await t.selectedNames(), ["b.txt", "c.txt"], "selection after switching back");

      await t.call("startSearch", "match");
      await t.waitFor("__e2e.app.search?.done", 10000, "search finished");
      await t.call("newTab", d);
      await t.settle();
      await t.call("switchTab", first);
      await t.waitFor("__e2e.app.search?.done", 10000, "search reran");
      await t.settle();
      t.eq(await t.names(), ["deep-match.txt"], "search results");
      t.eq(await t.ev("__e2e.app.filter"), "match", "search box text");
    },
  },
  {
    name: "a command acts on the item you selected even after a re-sort",
    async run(t) {
      const d = t.dir("stable-target");
      t.mk(path.join(d, "a.txt"), "1");
      t.mk(path.join(d, "b.txt"), "22222");
      t.mk(path.join(d, "c.txt"), "333");
      await t.nav(d);
      await t.select(["a.txt"]);
      await t.call("sortBy", "size", false); // a.txt moves from first to last
      await t.settle();
      t.eq(await t.names(), ["b.txt", "c.txt", "a.txt"], "re-sorted");
      t.eq(await t.selectedNames(), ["a.txt"], "selection followed the item");
      t.eq(await t.copyAsPath(), `"${path.join(d, "a.txt")}"`, "Copy as path target");
      // A selection from before the re-sort must be refused, not resolved to another row.
      const err = await t.ev(`__e2e.invoke("copy_as_path", { sel: { rev: __e2e.app.rev - 1, idx: [0] } }).then(() => "accepted", (e) => String(e))`);
      t.ok(err.startsWith("The folder changed"), `stale selection was ${err}`);
      await t.call("sortBy", "name", true);
    },
  },
  {
    name: "New, Paste and Select all do nothing on pages that are not folders",
    async run(t) {
      await t.nav("::Home");
      await t.key("N", { ctrl: true, shift: true });
      await t.key("v", { ctrl: true });
      await t.key("a", { ctrl: true });
      await t.sleep(300);
      t.eq(await t.ev("__e2e.app.status"), "", "no error from an impossible command");
      t.eq(await t.ev("__e2e.app.allSelected"), false, "nothing selected");
      t.eq(await t.ev("__e2e.app.path"), "::Home", "still on Home");
    },
  },
  {
    name: "a deleted current folder falls back to its parent",
    async run(t) {
      const d = t.dir("vanish");
      const gone = t.dir(path.join("vanish", "gone"));
      t.mk(path.join(gone, "f.txt"));
      t.mk(path.join(d, "keep.txt"));
      await t.nav(gone);
      (await import("node:fs")).rmSync(gone, { recursive: true, force: true });
      await t.waitFor(`__e2e.app.path === ${JSON.stringify(d)}`, 5000, "moved to the parent");
      await t.settle();
      t.eq(await t.names(), ["keep.txt"], "parent listing");
      t.ok((await t.ev("__e2e.app.status")).includes("can't find"), "says why");
    },
  },
];
