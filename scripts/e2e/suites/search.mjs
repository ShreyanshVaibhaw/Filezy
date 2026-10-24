import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// One shared tree big enough that a search is still running when a test acts:
// 200 folders x 60 subfolders, each with one file. Every 10th file matches "needle".
let big = null;
function bigTree(t) {
  if (big) return big;
  const root = t.dir("search-big");
  let matches = 0;
  for (let a = 0; a < 200; a++) {
    for (let b = 0; b < 60; b++) {
      const dir = path.join(root, `a${a}`, `b${b}`);
      fs.mkdirSync(dir, { recursive: true });
      const hit = (a * 60 + b) % 10 === 0;
      if (hit) matches++;
      fs.writeFileSync(path.join(dir, hit ? `needle-${a}-${b}.txt` : `hay-${a}-${b}.txt`), "");
    }
  }
  big = { root, matches };
  return big;
}

const done = (t) => t.waitFor("__e2e.app.search?.done", 30000, "search should finish");

export default [
  {
    name: "rapid query changes end with only the last query's results",
    async run(t) {
      const d = t.dir("search-rapid");
      for (const n of ["apple", "banana", "cherry"]) for (let i = 1; i <= 3; i++) t.mk(path.join(d, `sub${i}`, `${n}${i}.txt`));
      await t.nav(d);
      // Typing: each keystroke restarts the debounce; only the last runs.
      for (const q of ["a", "ap", "app", "b", "ba", "ch", "che", "cherry"]) await t.call("setFilter", q);
      await t.waitFor("__e2e.app.search?.query === 'cherry'", 5000, "debounced search started");
      await done(t);
      await t.settle();
      t.eq((await t.names()).sort(), ["cherry1.txt", "cherry2.txt", "cherry3.txt"], "results");
      // Starting searches back to back without waiting: only the newest survives.
      await t.ev(`(() => { __e2e.startSearch("apple"); __e2e.startSearch("banana"); })()`);
      await t.waitFor("__e2e.app.search?.query === 'banana' && __e2e.app.search.done", 10000, "banana finished");
      await t.settle();
      t.eq((await t.names()).sort(), ["banana1.txt", "banana2.txt", "banana3.txt"], "results after back-to-back searches");
    },
  },
  {
    name: "going Home mid-search stops it and nothing leaks back",
    async run(t) {
      const { root } = bigTree(t);
      await t.nav(root);
      await t.call("startSearch", "needle");
      await t.waitFor("__e2e.app.searching", 5000, "search running");
      await t.nav("::Home");
      await t.sleep(1500);
      t.eq(await t.ev("__e2e.app.path"), "::Home", "still Home");
      t.eq(await t.ev("__e2e.app.search"), null, "no search state on Home");
      t.eq(await t.ev("__e2e.app.total"), 0, "no rows on Home");
      await t.nav(root);
      t.eq(await t.ev("__e2e.app.total"), 200, "the folder itself, not search results");
    },
  },
  {
    name: "sorting while results stream keeps each result exactly once",
    async run(t) {
      const { root, matches } = bigTree(t);
      await t.nav(root);
      await t.call("startSearch", "needle");
      await t.waitFor("__e2e.app.searching && __e2e.app.total > 0", 10000, "results arriving");
      await t.call("sortBy", "name", false);
      await done(t);
      await t.settle();
      const names = await t.names();
      t.eq(names.length, matches, "every match once");
      t.eq(new Set(names).size, matches, "no duplicates");
      const sorted = [...names].sort((a, b) => b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" }));
      t.eq(names.slice(0, 5), sorted.slice(0, 5), "final order follows the chosen sort");
      await t.call("sortBy", "name", true);
    },
  },
  {
    name: "Stop keeps the results so far and says the search was stopped",
    async run(t) {
      const { root, matches } = bigTree(t);
      await t.nav(root);
      await t.call("startSearch", "needle");
      await t.waitFor("__e2e.app.searching && __e2e.app.total > 0", 10000, "results arriving");
      await t.click(".stopbtn");
      await done(t);
      await t.settle();
      t.ok(await t.ev("__e2e.app.search.stopped"), "marked stopped");
      const n = await t.ev("__e2e.app.total");
      t.ok(n > 0 && n < matches, `partial results (${n} of ${matches})`);
      t.ok((await t.ev("document.querySelector('.statusbar').textContent")).includes("Search stopped"), "status bar says so");
    },
  },
  {
    name: "a capped search says it is incomplete",
    async run(t) {
      // The runner caps results at 2,000 (FILEZY_E2E_SEARCH_MAX) so this stays quick.
      const d = t.dir("search-cap");
      for (let i = 0; i < 2100; i++) fs.writeFileSync(path.join(d, `match-${i}.txt`), "");
      await t.nav(path.dirname(d));
      await t.call("startSearch", "match-");
      await done(t);
      await t.settle();
      t.ok(await t.ev("__e2e.app.search.capped"), "marked capped");
      t.eq(await t.ev("__e2e.app.total"), 2000, "stopped at the cap");
      t.ok((await t.ev("document.querySelector('.statusbar').textContent")).includes("Showing the first 2,000 results"), "status bar says so");
    },
  },
  {
    name: "search follows the hidden-items setting",
    async run(t) {
      const d = t.dir("search-hidden");
      t.mk(path.join(d, "seen-target.txt"));
      const h = t.mk(path.join(d, "deep", "hidden-target.txt"));
      execFileSync("attrib.exe", ["+h", h], { windowsHide: true });
      await t.nav(d);
      await t.call("startSearch", "target");
      await done(t);
      await t.settle();
      t.eq(await t.names(), ["seen-target.txt"], "hidden result left out");
      await t.call("setSetting", "showHidden", true);
      try {
        await t.call("startSearch", "target", true);
        await done(t);
        await t.settle();
        t.eq((await t.names()).sort(), ["hidden-target.txt", "seen-target.txt"], "hidden result included");
      } finally {
        await t.call("setSetting", "showHidden", false);
      }
    },
  },
  {
    name: "results stay after a file changes and reflect creates, renames and deletes",
    async run(t) {
      const d = t.dir("search-fresh");
      const a = t.mk(path.join(d, "sub", "fresh-one.txt"), "1");
      await t.nav(d);
      await t.call("startSearch", "fresh");
      await done(t);
      await t.settle();
      t.eq(await t.names(), ["fresh-one.txt"], "first search");
      // A change in the searched folder must not drop the results for the plain folder.
      t.mk(path.join(d, "unrelated.txt"), "x");
      fs.writeFileSync(a, "changed");
      await t.sleep(600);
      t.eq(await t.ev("__e2e.app.search?.query"), "fresh", "still showing the search");
      fs.renameSync(a, path.join(d, "sub", "fresh-two.txt"));
      t.mk(path.join(d, "fresh-three.txt"));
      await t.call("refresh"); // F5 runs the search again
      await done(t);
      await t.settle();
      t.eq((await t.names()).sort(), ["fresh-three.txt", "fresh-two.txt"], "after rename and create");
      fs.rmSync(path.join(d, "fresh-three.txt"));
      await t.call("refresh");
      await done(t);
      await t.settle();
      t.eq(await t.names(), ["fresh-two.txt"], "after delete");
    },
  },
  {
    name: "a search in one tab never changes another tab's listing",
    async run(t) {
      const { root } = bigTree(t);
      const other = t.dir("search-other-tab");
      t.mk(path.join(other, "only.txt"));
      await t.nav(root);
      await t.call("startSearch", "needle");
      await t.waitFor("__e2e.app.searching", 5000, "search running");
      await t.call("newTab", other);
      await t.settle();
      await t.sleep(1500);
      t.eq(await t.ev("__e2e.app.path"), other, "second tab's folder");
      t.eq(await t.names(), ["only.txt"], "second tab's listing untouched");
      t.eq(await t.ev("__e2e.app.search"), null, "no search state leaked into the second tab");
    },
  },
  {
    name: "filters narrow results by kind, extension, size and dates outside the index",
    async run(t) {
      const d = t.dir("search-filters");
      const year = new Date().getFullYear();
      const notes = t.mk(path.join(d, "plan-notes.txt"), "x".repeat(100));
      t.mk(path.join(d, "sub", "plan-old.docx"), "x".repeat(20 * 1024), new Date(year - 1, 5, 15, 12));
      t.mk(path.join(d, "sub", "plan-photo.jpg"));
      t.mk(path.join(d, "budget.xlsx"), "x".repeat(2 * 1024 * 1024));
      await t.psCmd(`[IO.File]::SetCreationTime('${notes}', [datetime]::new(${year - 1}, 3, 1))`);
      await t.nav(d);
      const results = async (q) => {
        await t.call("startSearch", q, true);
        await done(t);
        await t.settle();
        return (await t.names()).sort();
      };
      t.eq(await results("plan"), ["plan-notes.txt", "plan-old.docx", "plan-photo.jpg"], "words");
      t.eq(await results("plan kind:picture"), ["plan-photo.jpg"], "kind");
      t.eq(await results("kind:document"), ["budget.xlsx", "plan-notes.txt", "plan-old.docx"], "documents");
      t.eq(await results("ext:.txt"), ["plan-notes.txt"], "extension");
      t.eq(await results("type:docx"), ["plan-old.docx"], "extension by another name, without the dot");
      t.eq(await results("size:empty"), ["plan-photo.jpg"], "size bucket");
      t.eq(await results("size:>1MB"), ["budget.xlsx"], "size comparison");
      t.eq(await results("datemodified:last year"), ["plan-old.docx"], "modified last year");
      t.eq(await results(`datemodified:${year - 1}-06-15`), ["plan-old.docx"], "modified on a date");
      t.eq(await results(`modified:>${year - 1}-12-31`), ["budget.xlsx", "plan-notes.txt", "plan-photo.jpg", "sub"], "modified after a date");
      t.eq(await results("datecreated:last year"), ["plan-notes.txt"], "created last year");
      // The temp folder is outside the index: the status bar says what wasn't searched.
      t.ok((await t.ev("document.querySelector('.statusbar').textContent")).includes("excluded from the Windows index"), "coverage note");
      // A property only the index knows can't be answered by the walk.
      await t.call("startSearch", "author:bob", true);
      t.eq(await t.ev("__e2e.app.status"), '"author:bob" needs the Windows index, which doesn\'t cover this location.', "unsupported filter");
    },
  },
  {
    name: "Search options sets filters and scope in the query, and Clear filters keeps the words",
    async run(t) {
      const d = t.dir("search-options");
      t.mk(path.join(d, "memo-top.txt"));
      t.mk(path.join(d, "memo-pic.png"));
      t.mk(path.join(d, "deeper", "memo-deep.txt"));
      await t.nav(d);
      await t.click('input[aria-label="Search"]');
      await t.type("memo");
      await t.key("Enter");
      await t.waitFor("__e2e.app.search?.query === 'memo' && __e2e.app.search.done", 10000, "search ran");
      await t.settle();
      const names = async () => (await t.names()).sort();
      t.eq(await names(), ["memo-deep.txt", "memo-pic.png", "memo-top.txt"], "all subfolders");
      const openOptions = () => t.click(".cbtn", { where: { has: "Search options" } });
      const pick = async (sub, item) => {
        await openOptions();
        if (sub) await t.hover(await t.center(".menu-item", { has: sub }));
        await t.click(sub ? ".submenu .menu-item" : ".menu-item", { where: { has: item } });
        await done(t);
        await t.settle();
      };
      await pick("Kind", "Picture");
      t.eq(await t.ev("document.querySelector('input[aria-label=Search]').value"), "memo kind:picture", "the filter reads as text in the box");
      t.eq(await names(), ["memo-pic.png"], "kind filter");
      // The menu marks the filter in effect.
      await openOptions();
      await t.hover(await t.center(".menu-item", { has: "Kind" }));
      t.eq(await t.ev(`[...document.querySelectorAll(".submenu [aria-checked=true]")].map((b) => b.querySelector(".mtext").textContent)`), ["Picture"], "checked kind");
      await t.click(".menu-item", { where: { has: "Clear filters" } });
      await done(t);
      await t.settle();
      t.eq(await t.ev("__e2e.app.filter"), "memo", "filters gone, words kept");
      t.eq((await names()).length, 3, "all results back");
      try {
        await pick(null, "Current folder");
        t.eq(await names(), ["memo-pic.png", "memo-top.txt"], "current folder only");
        t.eq(await t.ev("__e2e.settings.searchScope"), "folder", "scope saved");
        await pick(null, "All subfolders");
        t.eq((await names()).length, 3, "subfolders again");
      } finally {
        await t.call("setSearchScope", "subfolders");
      }
    },
  },
  {
    name: "recent searches keep committed searches, run again from the menu, and clear",
    async run(t) {
      await t.call("setSetting", "searchHistory", []);
      const d = t.dir("search-recent");
      t.mk(path.join(d, "alpha-1.txt"));
      t.mk(path.join(d, "beta-1.txt"));
      await t.nav(d);
      await t.click('input[aria-label="Search"]');
      await t.type("alpha");
      await t.key("Enter");
      await t.waitFor("__e2e.app.search?.query === 'alpha' && __e2e.app.search.done", 10000, "alpha searched");
      t.eq(await t.ev("[...__e2e.settings.searchHistory]"), ["alpha"], "Enter keeps the search");
      // A pause while typing searches but isn't a recent search yet...
      await t.call("setFilter", "beta");
      await t.waitFor("__e2e.app.search?.query === 'beta' && __e2e.app.search.done", 10000, "beta searched");
      t.eq(await t.ev("[...__e2e.settings.searchHistory]"), ["alpha"], "typing pause");
      // ...until focus leaves the box.
      const openOptions = () => t.click(".cbtn", { where: { has: "Search options" } });
      await openOptions();
      t.eq(await t.ev("[...__e2e.settings.searchHistory]"), ["beta", "alpha"], "leaving the box keeps it");
      await t.hover(await t.center(".menu-item", { has: "Recent searches" }));
      t.eq(await t.ev(`[...document.querySelectorAll(".submenu .menu-item")].map((b) => b.textContent.trim())`), ["beta", "alpha"], "newest first");
      await t.click(".submenu .menu-item", { where: { text: "alpha" } });
      await t.waitFor("__e2e.app.search?.query === 'alpha' && __e2e.app.search.done", 10000, "picked search ran");
      await t.settle();
      t.eq(await t.names(), ["alpha-1.txt"], "results for the picked search");
      t.eq(await t.ev("document.querySelector('input[aria-label=Search]').value"), "alpha", "search box shows it");
      // Clicking into the empty search box lists them too.
      await t.click('input[aria-label="Search"]');
      await t.key("Escape");
      await t.settle();
      await t.click('input[aria-label="Search"]');
      t.eq(await t.ev(`[...document.querySelectorAll(".recentdrop .menu-item")].map((b) => b.textContent.slice(1).trim())`), ["alpha", "beta"], "the empty box lists them");
      await t.click(".recentdrop .menu-item", { where: { has: "beta" } });
      await t.waitFor("__e2e.app.search?.query === 'beta' && __e2e.app.search.done", 10000, "picked from the box");
      await t.settle();
      t.eq(await t.names(), ["beta-1.txt"], "results for the search picked in the box");
      await openOptions();
      await t.click(".menu-item", { where: { has: "Clear search history" } });
      t.eq(await t.ev("[...__e2e.settings.searchHistory]"), [], "history cleared");
      await openOptions();
      await t.hover(await t.center(".menu-item", { has: "Recent searches" }));
      t.eq(
        await t.ev(`[...document.querySelectorAll(".submenu .menu-item")].map((b) => [b.textContent.trim(), b.disabled])`),
        [["No recent searches", true]],
        "empty history says so",
      );
      await t.key("Escape");
    },
  },
  {
    name: "searching every drive says what it covers and lasts one search",
    async run(t) {
      const d = t.dir("search-all");
      await t.nav(d);
      await t.call("setSearchScope", "all");
      try {
        await t.call("startSearch", `nothing-named-${t.token}`, true);
        await t.waitFor("__e2e.app.searching", 5000, "search running");
        await t.sleep(300);
        await t.click(".stopbtn");
        await done(t);
        t.ok((await t.ev("document.querySelector('.statusbar').textContent")).includes("file contents aren't searched across drives"), "coverage note");
        t.eq(await t.ev("__e2e.settings.searchScope"), "subfolders", "all drives isn't saved as the scope");
      } finally {
        await t.call("clearSearch");
        await t.settle();
      }
      t.eq(await t.ev("__e2e.app.searchScope"), "subfolders", "leaving the search ends it");
    },
  },
  {
    name: "indexed search finds words inside files and says the index answered",
    async run(t) {
      // The temp folder is never indexed; the profile is by default.
      const d = path.join(os.homedir(), `filezy-e2e-index-${t.token}`);
      const word = `zqxw${t.token}`;
      t.mk(path.join(d, "has-it.txt"), `the secret word is ${word} here`);
      t.mk(path.join(d, "without.txt"), "nothing to see");
      t.mk(path.join(d, "inner", "deep-has-it.txt"), `also ${word}`);
      const results = async (q) => {
        await t.call("startSearch", q, true);
        await done(t);
        await t.settle();
        return (await t.names()).sort();
      };
      try {
        await t.nav(d);
        // The indexer reads new files within seconds; ask until it has both.
        let names = [];
        for (const end = Date.now() + 90000; names.length < 2 && Date.now() < end; ) {
          names = await results(word);
          if ((await t.ev("__e2e.app.search.provider")) !== "index") t.skip(await t.ev("__e2e.app.search.note"));
          if (names.length < 2) await t.sleep(1000);
        }
        t.eq(names, ["deep-has-it.txt", "has-it.txt"], "words inside files, in every subfolder");
        t.ok((await t.ev("document.querySelector('.statusbar').textContent")).includes("Results from the Windows index"), "status bar");
        t.eq(await results("kind:text"), ["deep-has-it.txt", "has-it.txt", "without.txt"], "Advanced Query Syntax");
        await t.call("setSearchScope", "folder");
        t.eq(await results(word), ["has-it.txt"], "current folder only");
      } finally {
        await t.call("setSearchScope", "subfolders");
        await t.nav(t.fixRoot);
        fs.rmSync(d, { recursive: true, force: true });
      }
    },
  },
  {
    name: "renaming inside search results keeps the results and selects the renamed item",
    async run(t) {
      const d = t.dir("search-rename");
      t.mk(path.join(d, "sub", "todo.txt"), "t");
      t.mk(path.join(d, "other.txt"), "o");
      t.mk(path.join(d, "unrelated.md"), "u");
      await t.nav(d);
      await t.call("startSearch", "txt");
      await t.waitFor("__e2e.app.search?.done", 30000, "search finished");
      await t.settle();
      t.eq((await t.names()).sort(), ["other.txt", "todo.txt"], "the results");
      await t.select(["todo.txt"]);
      await t.ev(`__e2e.commitRename([...__e2e.sel][0], "done.txt")`);
      await t.waitFor("__e2e.app.search?.done && __e2e.idle()", 30000, "the search ran again");
      t.ok(fs.existsSync(path.join(d, "sub", "done.txt")), "renamed on disk");
      t.eq(await t.ev("__e2e.app.search?.query"), "txt", "still a search");
      t.eq((await t.names()).sort(), ["done.txt", "other.txt"], "the results, not the folder");
      t.eq(await t.selectedNames(), ["done.txt"], "the renamed item is selected");
    },
  },
];
