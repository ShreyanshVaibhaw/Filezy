// The Details view's columns, dates and sort order as Windows has them, and
// the Details pane.
import path from "node:path";

const headers = (t) => t.ev(`[...document.querySelectorAll(".header .hcell .hlabel")].map((e) => e.textContent)`);

const headerOf = (label) =>
  `[...document.querySelectorAll(".header .hcell")].find((c) => c.querySelector(".hlabel")?.textContent === ${JSON.stringify(label)})`;

const colWidth = (t, label) => t.ev(`Math.round(${headerOf(label)}?.getBoundingClientRect().width ?? 0)`);

/** A row's text in the column with this header. */
const cell = (t, name, label) =>
  t.ev(`(() => {
    const labels = [...document.querySelectorAll(".header .hcell .hlabel")].map((e) => e.textContent);
    const i = labels.indexOf(${JSON.stringify(label)});
    const row = [...document.querySelectorAll(".vrow")].find((r) => r.querySelector(".fname")?.textContent === ${JSON.stringify(name)});
    return row && i > 0 ? row.querySelectorAll(".cell")[i].textContent : null;
  })()`);

/** Is any cell in the column with this header cut off (wider than its cell)? */
const clipped = (t, label) =>
  t.ev(`(() => {
    const labels = [...document.querySelectorAll(".header .hcell .hlabel")].map((e) => e.textContent);
    const i = labels.indexOf(${JSON.stringify(label)});
    return [...document.querySelectorAll(".vrow")].some((r) => { const c = r.querySelectorAll(".cell")[i]; return c && c.scrollWidth > c.clientWidth + 1; });
  })()`);

/** Right-click a header and pick an entry from its menu. */
async function headerMenu(t, label, entry) {
  const at = await t.ev(`(() => { const r = ${headerOf(label)}.getBoundingClientRect(); return { x: r.left + 30, y: r.top + r.height / 2 }; })()`);
  const under = await t.ev(`document.elementFromPoint(${at.x}, ${at.y})?.closest(".hcell")?.querySelector(".hlabel")?.textContent`);
  if (under !== label) throw new Error(`the ${label} header isn't at ${JSON.stringify(at)} (${under} is): the layout was still moving`);
  await t.click(at, { button: "right" });
  await t.waitFor("!!document.querySelector('.hdrmenu')", 3000, "the header menu");
  await t.ev(`[...document.querySelectorAll(".hdrmenu .menu-item")].find((b) => b.textContent.trim() === ${JSON.stringify(entry)}).click()`);
  await t.sleep(100);
}

const photo = (t, file, tags, title) => t.ps("photo.ps1", "-Path", file, "-Tags", tags, ...(title ? ["-Title", title] : []));
const tagsOf = (t, file) => t.ps("photo.ps1", "-Path", file, "-Read");

export default [
  {
    name: "chosen columns, widths and order stay with their folder across a restart",
    async run(t) {
      const a = t.dir("cols-a");
      const b = t.dir("cols-b");
      t.mk(path.join(a, "x.txt"), "x");
      t.mk(path.join(b, "y.txt"), "y");
      await t.nav(a);
      await headerMenu(t, "Type", "Date created");
      t.eq(await headers(t), ["Name", "Date modified", "Type", "Size", "Date created"], "added at the end");
      const grip = await t.ev(`(() => { const r = ${headerOf("Type")}.querySelector(".hresize").getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
      await t.pageDrag(grip, { x: grip.x + 60, y: grip.y }, 6);
      await t.until(async () => Math.abs((await colWidth(t, "Type")) - 210) <= 3, 3000, "Type widened by the drag");
      const typeW = await colWidth(t, "Type");
      const from = await t.center(".header .hcell", { has: "Date created" });
      const to = await t.ev(`(() => { const r = ${headerOf("Date modified")}.getBoundingClientRect(); return { x: r.left + 10, y: r.top + r.height / 2 }; })()`);
      await t.pageDrag(from, to, 6);
      t.eq(await headers(t), ["Name", "Date created", "Date modified", "Type", "Size"], "moved before Date modified");
      await t.restart();
      await t.nav(a);
      t.eq(await headers(t), ["Name", "Date created", "Date modified", "Type", "Size"], "the layout came back");
      t.ok(Math.abs((await colWidth(t, "Type")) - typeW) <= 2, "and its widths");
      await t.nav(b);
      t.eq(await headers(t), ["Name", "Date modified", "Type", "Size"], "another folder keeps the default");
    },
  },
  {
    name: "Size column to fit and Size all columns to fit leave no text cut off",
    async run(t) {
      const d = t.dir("cols-fit");
      t.mk(path.join(d, "doc.txt"), "d");
      t.dir("cols-fit/A folder");
      await t.nav(d);
      // Type names arrive a moment after the rows; fit what the column will show.
      await t.waitFor(`!!__e2e.extMeta.get("txt")?.type_name && !!__e2e.extMeta.get("//dir")?.type_name`, 5000, "the type names");
      const grip = await t.ev(`(() => { const r = ${headerOf("Type")}.querySelector(".hresize").getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
      await t.pageDrag(grip, { x: grip.x - 100, y: grip.y }, 6);
      // The page applies the drag's moves over the next frames: wait for where it ends.
      await t.until(async () => (await colWidth(t, "Type")) === 50, 3000, "Type narrowed to its minimum");
      t.ok(await clipped(t, "Type"), "the type names don't fit");
      await headerMenu(t, "Type", "Size column to fit");
      await t.until(async () => !(await clipped(t, "Type")), 3000, "Type fits its text again");
      await t.pageDrag(grip, { x: grip.x - 100, y: grip.y }, 6);
      await t.until(async () => (await colWidth(t, "Type")) === 50, 3000, "Type narrowed again");
      await headerMenu(t, "Size", "Size all columns to fit");
      for (const label of ["Date modified", "Type", "Size"]) await t.until(async () => !(await clipped(t, label)), 3000, `${label} fits`);
    },
  },
  {
    name: "Tags and Title columns show each file's properties; an outside change shows on refresh and re-sorts",
    async run(t) {
      const d = t.dir("cols-props");
      await photo(t, path.join(d, "a.jpg"), "zulu", "Alpha");
      await photo(t, path.join(d, "b.jpg"), "yankee", "Bravo");
      await photo(t, path.join(d, "c.jpg"), "", "Charlie");
      await t.nav(d);
      await headerMenu(t, "Type", "Tags");
      await headerMenu(t, "Type", "Title");
      await t.until(async () => (await cell(t, "a.jpg", "Tags")) === "zulu" && (await cell(t, "b.jpg", "Title")) === "Bravo", 10000, "property values");
      await t.call("sortBy", "tags", true);
      await t.settle();
      // A blank value is the smallest, as in Windows' property compare.
      t.eq(await t.names(), ["c.jpg", "b.jpg", "a.jpg"], "no tags, then yankee, then zulu");
      await t.call("sortBy", "tags", false);
      await t.settle();
      t.eq(await t.names(), ["a.jpg", "b.jpg", "c.jpg"], "reversed whole when descending");
      await t.call("sortBy", "tags", true);
      await t.settle();
      await photo(t, path.join(d, "a.jpg"), "able", "Alpha");
      await t.call("refresh");
      await t.settle();
      await t.until(async () => (await cell(t, "a.jpg", "Tags")) === "able", 10000, "the new tag shows");
      t.eq(await t.names(), ["c.jpg", "a.jpg", "b.jpg"], "and the order follows it");
    },
  },
  {
    name: "dates use Windows' short date and time format, and names sort as Windows sorts them",
    async run(t) {
      const d = t.dir("cols-locale");
      t.mk(path.join(d, "dated.txt"), "d", new Date(2024, 2, 5, 14, 7));
      for (const n of ["Zebra.txt", "école.txt", "File10.txt", "file2.txt"]) t.mk(path.join(d, n), n);
      await t.nav(d);
      const want = (
        await t.psCmd(
          "$f = (Get-Culture).DateTimeFormat; (Get-Date -Year 2024 -Month 3 -Day 5 -Hour 14 -Minute 7 -Second 0).ToString($f.ShortDatePattern + ' ' + $f.ShortTimePattern)",
        )
      ).trim();
      t.eq(await cell(t, "dated.txt", "Date modified"), want, "the date reads as Windows formats it");
      t.eq(await t.names(), ["dated.txt", "école.txt", "file2.txt", "File10.txt", "Zebra.txt"], "accents in place, numbers by value");
    },
  },
  {
    name: "the Details pane edits a photo's tags and says which selected files couldn't take them",
    async run(t) {
      const d = t.dir("dpane");
      const jpg = path.join(d, "photo.jpg");
      await photo(t, jpg, "old");
      t.mk(path.join(d, "notes.txt"), "n");
      await t.nav(d);
      await t.call("setSetting", "detailsPane", true);
      try {
        const editTags = async (value) => {
          // The field turns editable once the pane knows the file can store tags.
          await t.waitFor(`!!document.querySelector('.details dd .value[title="Edit Tags"]')`, 5000, "an editable Tags field");
          await t.ev(`document.querySelector('.details dd .value[title="Edit Tags"]').click()`);
          await t.waitFor("document.activeElement?.classList.contains('edit')", 3000, "the tags box");
          await t.ev(`(() => { const i = document.activeElement; i.value = ${JSON.stringify(value)}; i.dispatchEvent(new Event("input", { bubbles: true })); })()`);
          await t.key("Enter");
        };
        await t.select(["photo.jpg"]);
        await t.waitFor(`document.querySelector(".details .name")?.textContent === "photo.jpg"`, 5000, "the photo in the pane");
        await editTags("red; blue");
        await t.until(async () => (await tagsOf(t, jpg)) === "red;blue", 10000, "the tags were written to the file");
        await t.settle();
        await t.select(["notes.txt", "photo.jpg"]);
        await t.waitFor(`document.querySelector(".details .name")?.textContent === "2 items selected"`, 5000, "both in the pane");
        await editTags("green");
        await t.until(async () => (await tagsOf(t, jpg)) === "green", 10000, "the photo took the new tag");
        const status = await t.ev("__e2e.app.status");
        t.ok(status.includes("1 of 2") && status.includes("notes.txt"), status);
      } finally {
        await t.call("setSetting", "detailsPane", false);
      }
    },
  },
  {
    name: "Alt+P and Alt+Shift+P switch between the Preview and Details panes, and the pane's edge resizes from the keyboard",
    async run(t) {
      const d = t.dir("panes");
      t.mk(path.join(d, "a.txt"), "a");
      await t.nav(d);
      await t.call("setSetting", "paneWidth", 300);
      try {
        await t.key("p", { alt: true, shift: true });
        await t.waitFor("!!document.querySelector('.details') && !document.querySelector('.preview')", 3000, "the Details pane");
        await t.key("p", { alt: true });
        await t.waitFor("!!document.querySelector('.preview') && !document.querySelector('.details')", 3000, "the Preview pane instead");
        await t.ev("document.querySelector('.panegrip .splitter').focus()");
        await t.key("ArrowLeft");
        await t.key("ArrowLeft");
        t.eq(await t.ev("__e2e.settings.paneWidth"), 332, "two steps wider, and saved");
        await t.until(async () => (await t.ev("Math.round(document.querySelector('.sidepane').getBoundingClientRect().width)")) === 332, 3000, "drawn that wide");
        await t.key("p", { alt: true });
        await t.waitFor("!document.querySelector('.sidepane')", 3000, "no pane");
      } finally {
        await t.call("setSetting", "previewPane", false);
        await t.call("setSetting", "detailsPane", false);
        await t.call("setSetting", "paneWidth", 300);
      }
    },
  },
  {
    name: "the Details pane shows where a shortcut points",
    async run(t) {
      const d = t.dir("dpane-link");
      const target = t.mk(path.join(d, "doc.txt"), "d");
      await t.nav(d);
      await t.select(["doc.txt"]);
      await t.call("createShortcuts");
      await t.sleep(400);
      await t.settle();
      await t.call("setSetting", "detailsPane", true);
      try {
        await t.select(["doc.txt - Shortcut.lnk"]);
        await t.waitFor(
          `[...document.querySelectorAll(".details dt")].find((x) => x.textContent === "Target")?.nextElementSibling?.textContent === ${JSON.stringify(target)}`,
          // Resolving a shortcut goes through the shell, which can be slow on a busy machine.
          10000,
          "the target",
        );
      } finally {
        await t.call("setSetting", "detailsPane", false);
      }
    },
  },
];
