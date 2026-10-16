import path from "node:path";

// These tests use the real Recycle Bin. Every file name carries the run's token,
// so selections and restores can only match items this run created, and the
// runner restores leftovers at the end.

async function recycle(t, file) {
  await t.nav(path.dirname(file));
  await t.select([path.basename(file)]);
  await t.call("del");
  await t.until(() => !t.exists(file), 5000, `${file} was not recycled`);
  await t.settle();
}

const binCount = (t, name) =>
  t.ev(`__e2e.invoke("recycle_list").then((l) => l.items.filter((i) => i.name === ${JSON.stringify(name)}).length)`);

export default [
  {
    name: "restoring one of two same-named items restores only that one",
    async run(t) {
      const d = t.dir("bin-dup-names");
      const name = `report-${t.token}.txt`;
      const a = t.mk(path.join(d, "A", name), "from A");
      const b = t.mk(path.join(d, "B", name), "from B");
      await recycle(t, a);
      await recycle(t, b);

      await t.nav("::RecycleBin");
      t.eq(await binCount(t, name), 2, "both in the bin");
      await t.waitFor(`document.querySelectorAll(".rrow").length > 0`, 10000, "bin page never listed");
      await t.click(".rrow", { where: { title: path.dirname(a) } });
      await t.click(".abtn", { where: { has: "Restore selected" } });
      await t.until(() => t.exists(a), 10000, "the selected item was not restored");
      t.eq(t.read(a), "from A", "restored content");
      await t.sleep(500);
      t.ok(!t.exists(b), "the other same-named item was restored too");
      t.eq(await binCount(t, name), 1, "the other stays in the bin");
    },
  },
  {
    name: "undo of a delete brings back exactly the deleted item",
    async run(t) {
      const d = t.dir("bin-undo");
      const name = `plan-${t.token}.txt`;
      const a = t.mk(path.join(d, "A", name), "A");
      const b = t.mk(path.join(d, "B", name), "B");
      await recycle(t, a);
      await recycle(t, b);
      await t.call("undo");
      await t.until(() => t.exists(b), 10000, "undo restores the last delete");
      await t.sleep(300);
      t.ok(!t.exists(a), "the earlier same-named delete stays in the bin");
      await t.call("undo");
      await t.until(() => t.exists(a), 10000, "a second undo restores the earlier one");
      t.eq([t.read(a), t.read(b)], ["A", "B"], "each came back to its own folder");
    },
  },
  {
    name: "Delete selected in the bin asks Windows and removes only the selection",
    async run(t) {
      const d = t.dir("bin-delete-sel");
      const keep = `keep-${t.token}.txt`;
      const drop = `drop-${t.token}.txt`;
      await recycle(t, t.mk(path.join(d, keep), "k"));
      await recycle(t, t.mk(path.join(d, drop), "d"));
      await t.nav("::RecycleBin");
      await t.waitFor(`[...document.querySelectorAll(".rrow")].some((r) => r.textContent.includes(${JSON.stringify(drop)}))`, 10000, "listed");
      const row = await t.ev(`(() => {
        const r = [...document.querySelectorAll(".rrow")].find((r) => r.textContent.includes(${JSON.stringify(drop)}));
        const b = r.getBoundingClientRect();
        return { x: b.left + 40, y: b.top + b.height / 2 };
      })()`);
      await t.click(row);
      await t.ev(`void [...document.querySelectorAll(".abtn")].find((b) => b.textContent.includes("Delete selected")).click()`);
      await t.uia("-Window", "*Delete*", "-Command", "6"); // Yes
      await t.until(async () => (await binCount(t, drop)) === 0, 10000, "the selected item should be gone from the bin");
      t.eq(await binCount(t, keep), 1, "the unselected item stays");
      t.ok(!t.exists(path.join(d, drop)), "and it was not restored");
    },
  },
];
