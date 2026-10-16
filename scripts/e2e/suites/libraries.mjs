// Libraries (F10). Each test makes its own .library-ms in its fixture folder;
// the user's own libraries are never read or changed.
import fs from "node:fs";
import path from "node:path";

/** A library file naming `folders`, the first one being where new items go. */
function library(file, folders) {
  const locations = folders
    .map((f, i) => `<searchConnectorDescription>${i === 0 ? "<isDefaultSaveLocation>true</isDefaultSaveLocation>" : ""}<simpleLocation><url>${f}</url></simpleLocation></searchConnectorDescription>`)
    .join("");
  fs.writeFileSync(
    file,
    `<?xml version="1.0" encoding="UTF-8"?><libraryDescription xmlns="http://schemas.microsoft.com/windows/2009/library"><name>${path.basename(file, ".library-ms")}</name><version>1</version><isLibraryPinned>false</isLibraryPinned><searchConnectorDescriptionList>${locations}</searchConnectorDescriptionList></libraryDescription>`,
  );
  return file;
}

/** Each row's name and the real folder it lives in. */
const rows = (t) => t.ev("__e2e.listing().then((r) => r.map((e) => [e.name, e.parent]))");

export default [
  {
    name: "a library opens as its folders' items together, each under its real folder, and follows changes in any of them",
    async run(t) {
      const d = t.dir("lib-browse");
      const first = t.dir("lib-browse/first");
      const second = t.dir("lib-browse/second");
      t.mk(path.join(first, "a.txt"), "a");
      t.mk(path.join(second, "b.txt"), "b");
      const lib = library(path.join(d, "Work.library-ms"), [first, second]);
      // Opened the way a double-click opens it.
      await t.nav(d);
      await t.call("openPath", lib);
      await t.waitFor(`__e2e.app.path.toLowerCase() === ${JSON.stringify(lib.toLowerCase())}`, 5000, "inside the library");
      await t.settle();
      const seen = await rows(t);
      t.eq(seen.map(([n]) => n).sort(), ["a.txt", "b.txt"], "both folders' items");
      t.eq(seen.find(([n]) => n === "b.txt")[1].toLowerCase(), second.toLowerCase(), "b.txt is under its real folder");
      t.eq(await t.ev(`[...document.querySelectorAll("button.crumb:not(.chevbtn)")].map((c) => c.textContent.trim()).slice(-2)`), ["lib-browse", "Work"], "the breadcrumbs: the folder holding the library, then the library");
      // A file appearing in the second folder shows up without a refresh.
      t.mk(path.join(second, "c.txt"), "c");
      await t.until(async () => (await t.names()).includes("c.txt"), 5000, "c.txt appeared");
    },
  },
  {
    name: "in a library, New folder goes to its save location and search covers every folder",
    async run(t) {
      const d = t.dir("lib-new");
      const first = t.dir("lib-new/first");
      const second = t.dir("lib-new/second");
      t.mk(path.join(first, "needle-one.txt"), "1");
      t.mk(path.join(second, "deep", "needle-two.txt"), "2");
      const lib = library(path.join(d, "Mixed.library-ms"), [first, second]);
      await t.nav(lib);
      await t.call("newFolder");
      await t.until(() => fs.existsSync(path.join(first, "New folder")), 5000, "New folder in the save location");
      t.ok(!fs.existsSync(path.join(second, "New folder")), "and only there");
      await t.until(async () => (await t.names()).includes("New folder"), 5000, "listed in the library");
      await t.waitFor("__e2e.app.renaming >= 0", 5000, "its name is ready to type, as after New folder anywhere");
      await t.ev("__e2e.app.renaming = -1");
      await t.call("startSearch", "needle");
      await t.waitFor("__e2e.app.search?.done", 30000, "search finished");
      await t.settle();
      t.eq((await t.names()).sort(), ["needle-one.txt", "needle-two.txt"], "found in both folders, subfolders included");
    },
  },
];
