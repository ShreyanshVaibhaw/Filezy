// Cloud folders (F10). The first test only reads: nothing in the user's own
// cloud folders is opened, listed or changed, and no file names are printed.
// The others run against a stand-in provider (src-tauri/examples/testcloud.rs)
// that registers a sync root in a fixture folder and unregisters it after
// each test. No real mouse or keyboard input is sent.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { E2E_TARGET } from "../../build-e2e.mjs";

const PROVIDER = path.join(E2E_TARGET, "debug", "examples", "testcloud.exe");
/** Every test sync root's id starts with this, so a crashed run's can be found. */
const PREFIX = "FilezyE2E!";

/** The sync roots Windows reports for this user, asked independently of Filezy. */
function syncRoots() {
  const script =
    "$m = [Windows.Storage.Provider.StorageProviderSyncRootManager,Windows.Storage,ContentType=WindowsRuntime]; " +
    "ConvertTo-Json -Compress -InputObject @($m::GetCurrentSyncRoots() | ForEach-Object { @{ name = $_.DisplayNameResource; path = $_.Path.Path } })";
  const out = execFileSync("powershell.exe", ["-NoProfile", "-Command", script], { windowsHide: true, encoding: "utf8" }).trim();
  return (out ? JSON.parse(out) : []).filter((r) => fs.existsSync(r.path));
}

/**
 * Start the stand-in provider in a fresh fixture folder. Its files:
 * online.txt, broken.txt (downloads fail) and slow.txt (downloads take two
 * seconds) only online; local.txt downloaded; pinned.txt kept on this device;
 * pending.txt changed here and not uploaded; docs\inner.txt only online.
 */
async function testCloud(t, rel) {
  if (!fs.existsSync(PROVIDER)) throw new Error(`missing ${PROVIDER}\nbuild it with: npm run build:e2e`);
  execFileSync(PROVIDER, ["clean", PREFIX], { windowsHide: true });
  const sid = execFileSync("whoami.exe", ["/user", "/fo", "csv", "/nh"], { windowsHide: true, encoding: "utf8" }).trim().split(",").pop().replace(/"/g, "");
  const dir = t.dir(rel);
  const p = spawn(PROVIDER, ["start", `${PREFIX}${sid}!${t.token}`, dir], { windowsHide: true });
  let err = "";
  p.stderr.on("data", (d) => (err += d));
  const exited = new Promise((_, reject) => p.once("exit", (c) => reject(new Error(`the test provider exited (${c}): ${err.trim()}`))));
  exited.catch(() => {});
  const lines = readline.createInterface({ input: p.stdout })[Symbol.asyncIterator]();
  const line = () => Promise.race([lines.next().then((r) => r.value), exited]);
  const first = await line();
  if (first !== "ready") throw new Error(`the test provider said ${first}: ${err.trim()}`);
  return {
    dir,
    /** A provider command (testcloud.rs), answered once done. */
    async send(cmd) {
      p.stdin.write(cmd + "\n");
      await line();
    },
    async stop() {
      p.stdin.end("quit\n");
      await exited.catch(() => {});
      // Unregisters the root even when the provider couldn't.
      execFileSync(PROVIDER, ["clean", PREFIX], { windowsHide: true });
    },
  };
}

/** Each row's Status, as the Status column shows it, by name. */
const statuses = (t) =>
  t.ev(`__e2e.listing().then((r) => Object.fromEntries(r.map((e, i) => [e.name, document.querySelector("#item-" + i + " .cell.status")?.title ?? ""])))`);

/** Wait for the Status column to show `want` (name -> text) for those rows. */
async function statusesAre(t, want, timeout = 10000) {
  const end = Date.now() + timeout;
  for (;;) {
    const seen = await statuses(t);
    if (Object.entries(want).every(([n, s]) => seen[n] === s)) return;
    if (Date.now() > end) throw new Error(`the Status column: expected ${JSON.stringify(want)}, saw ${JSON.stringify(seen)}`);
    await t.sleep(100);
  }
}
const statusIs = (t, name, want) => statusesAre(t, { [name]: want });

/** The file's attributes on disk, read by Windows (Node can't). */
async function attrs(t, file) {
  return Number(await t.psCmd(`[int](Get-Item -Force -LiteralPath '${file}').Attributes`));
}
const PINNED = 0x80000;
const ONLINE_ONLY = 0x400000;

/**
 * Click the preview pane's link once it reads `label`. Found and clicked in
 * one evaluation, so a newer load replacing the message can't come between.
 */
const clickLink = (t, label) =>
  t.waitFor(
    `(() => { const b = [...document.querySelectorAll(".preview .msg button.link")].find((b) => b.textContent.trim() === ${JSON.stringify(label)}); b?.click(); return !!b; })()`,
    5000,
    `the preview's "${label}"`,
  );

/** Open a row's menu (as Shift+F10 does) and pick an entry; returns its aria-checked. */
async function menuPick(t, name, label) {
  await t.select([name]);
  await t.ev(`__e2e.app.focus = ${(await t.names()).indexOf(name)}`);
  await t.ev("__e2e.hooks.contextMenu()");
  // The shell's entries arrive after the menu opens and push Filezy's own
  // down: click only once they are in, or the click lands on another entry.
  await t.waitFor(`!!document.querySelector(".ctxmenu") && !document.querySelector(".ctxmenu .menuload")`, 60000, "the item menu with the shell's entries");
  const checked = await t.ev(
    `[...document.querySelectorAll(".ctxmenu .menu-item")].find((b) => b.textContent.includes(${JSON.stringify(label)}))?.getAttribute("aria-checked")`,
  );
  await t.click(".ctxmenu .menu-item", { where: { has: label } });
  return checked;
}

export default [
  {
    name: "every cloud folder Windows has registered is in the navigation pane, under the name Windows gives it",
    async run(t) {
      const want = syncRoots();
      if (!want.length) t.skip("no cloud provider has registered a folder here");
      await t.waitFor(`__e2e.app.cloudRoots.length >= ${want.length}`, 10000, "the cloud folders loaded");
      const shown = await t.ev(
        `[...document.querySelectorAll('[aria-label="Navigation pane"] [data-key]')].map((b) => ({ path: b.dataset.key.toLowerCase(), label: b.querySelector(".label")?.textContent }))`,
      );
      want.forEach((r, i) => {
        const hit = shown.find((s) => s.path === r.path.toLowerCase());
        t.ok(hit, `cloud folder ${i + 1} of ${want.length} is in the navigation pane`);
        // A name that is a resource reference ("@...") is resolved by Filezy, not here.
        if (!r.name.startsWith("@")) t.eq(hit.label, r.name, `cloud folder ${i + 1} is named as Windows names it`);
      });
    },
  },
  {
    name: "in a cloud folder, Status comes first and says what Explorer says for each item, and sorts",
    async run(t) {
      const c = await testCloud(t, "cloud-states");
      try {
        await t.nav(c.dir);
        await t.waitFor("__e2e.app.cloud", 5000, "Filezy knows the folder is synced");
        t.eq(await t.ev(`[...document.querySelectorAll(".hcell[data-meta]")].map((h) => h.dataset.meta)`), ["status", "modified", "type", "size"], "the columns");
        const want = {
          docs: "Available when online",
          "local.txt": "Available on this device",
          "online.txt": "Available when online",
          "pending.txt": "Sync pending",
          "pinned.txt": "Always available on this device",
        };
        await statusesAre(t, want, 5000);
        // Reading the states changes nothing on disk, so the folder isn't listed again and again.
        const rev = await t.ev("__e2e.app.rev");
        await t.sleep(1500);
        t.eq(await t.ev("__e2e.app.rev"), rev, "the listing stayed as it was while nothing changed");
        const tone = (n) => t.ev(`__e2e.listing().then((r) => document.querySelector("#item-" + r.findIndex((e) => e.name === ${JSON.stringify(n)}) + " .cell.status .glyph")?.className)`);
        t.ok((await tone("online.txt")).includes("cloud"), "a cloud for an online-only file");
        t.ok((await tone("pinned.txt")).includes("ok"), "a check for a kept one");
        // Sorted by Status: folders first, then the wording in order.
        await t.click(`.hcell[data-meta="status"]`);
        await t.waitFor(`__e2e.app.sortKey === "status"`, 3000, "sorted by Status");
        await t.settle();
        t.eq(await t.names(), ["docs", "pinned.txt", "local.txt", "broken.txt", "online.txt", "slow.txt", "pending.txt"], "the order");
        await t.call("sortBy", "name");
        // The Details pane says it too, under Explorer's label.
        await t.call("setSetting", "detailsPane", true);
        try {
          await t.select(["pending.txt"]);
          await t.waitFor(
            `[...document.querySelectorAll(".details dt")].some((d) => d.textContent === "Availability status" && d.nextElementSibling.textContent === "Sync pending")`,
            5000,
            "Availability status in the Details pane",
          );
        } finally {
          await t.call("setSetting", "detailsPane", false);
        }
      } finally {
        await c.stop();
      }
    },
  },
  {
    name: "Always keep on this device and Free up space download and free files, and the Status follows",
    async run(t) {
      const c = await testCloud(t, "cloud-pin");
      try {
        await t.nav(c.dir);
        await statusIs(t, "online.txt", "Available when online");
        const online = path.join(c.dir, "online.txt");
        t.eq(await menuPick(t, "online.txt", "Always keep on this device"), "false", "not kept yet");
        await statusIs(t, "online.txt", "Always available on this device");
        const a = await attrs(t, online);
        t.ok(a & PINNED && !(a & ONLINE_ONLY), `kept and downloaded (attributes 0x${a.toString(16)})`);
        // A second time stops keeping it, as in Explorer; it stays downloaded.
        t.eq(await menuPick(t, "online.txt", "Always keep on this device"), "true", "shown as kept");
        await statusIs(t, "online.txt", "Available on this device");
        const local = path.join(c.dir, "local.txt");
        await menuPick(t, "local.txt", "Free up space");
        await statusIs(t, "local.txt", "Available when online");
        t.ok((await attrs(t, local)) & ONLINE_ONLY, "its data is gone from this device");
        // A folder: everything in it is kept.
        await menuPick(t, "docs", "Always keep on this device");
        await t.until(async () => !((await attrs(t, path.join(c.dir, "docs", "inner.txt"))) & ONLINE_ONLY), 10000, "the file in the folder was downloaded");
      } finally {
        await c.stop();
      }
    },
  },
  {
    name: "a file the provider finishes uploading turns from Sync pending to on this device without a refresh",
    async run(t) {
      const c = await testCloud(t, "cloud-sync");
      try {
        await t.nav(c.dir);
        await statusIs(t, "pending.txt", "Sync pending");
        await c.send("insync pending.txt");
        await statusIs(t, "pending.txt", "Available on this device");
      } finally {
        await c.stop();
      }
    },
  },
  {
    name: "previewing an online-only file shows the download, and a failed one says why and can be tried again",
    async run(t) {
      const c = await testCloud(t, "cloud-preview");
      await t.call("setSetting", "detailsPane", false);
      await t.call("setSetting", "previewPane", true);
      const msg = `document.querySelector(".preview .msg")?.textContent.trim() ?? ""`;
      try {
        await t.nav(c.dir);
        await t.select(["slow.txt"]);
        await clickLink(t, "Download and preview");
        await t.waitFor(`${msg} === "Downloading…"`, 3000, "says it's downloading while the provider takes its time");
        await t.waitFor(`document.querySelector(".preview .text")?.textContent.startsWith("slow.txt")`, 10000, "then shows it");
        await t.select(["broken.txt"]);
        await clickLink(t, "Download and preview");
        await t.waitFor(`${msg}.startsWith("This file couldn't be downloaded.")`, 10000, "says the download failed");
        t.ok((await attrs(t, path.join(c.dir, "broken.txt"))) & ONLINE_ONLY, "nothing was half-downloaded");
        // The provider recovers; Try again gets it.
        await c.send("fix broken.txt");
        await clickLink(t, "Try again");
        await t.waitFor(`document.querySelector(".preview .text")?.textContent.startsWith("broken.txt")`, 10000, "shown after trying again");
      } finally {
        await t.call("setSetting", "previewPane", false);
        await c.stop();
      }
    },
  },
];
