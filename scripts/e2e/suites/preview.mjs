// The preview pane (F08). Everything here goes through the page and the file
// system; no real mouse or keyboard input is sent. Windows' own previewers
// are checked from outside with preview.ps1, which only reads window state.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const TEXT = ".preview .text";
const MSG = ".preview .msg";

/** A ZIP of stored (uncompressed) entries: enough for an Office document. */
function zip(files) {
  const parts = [];
  const central = [];
  let offset = 0;
  for (const [name, data] of Object.entries(files)) {
    const body = Buffer.from(data);
    const n = Buffer.from(name);
    const crc = zlib.crc32(body);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(n.length, 26);
    parts.push(local, n, body);
    const dir = Buffer.alloc(46);
    dir.writeUInt32LE(0x02014b50, 0);
    dir.writeUInt16LE(20, 4);
    dir.writeUInt16LE(20, 6);
    dir.writeUInt32LE(crc, 16);
    dir.writeUInt32LE(body.length, 20);
    dir.writeUInt32LE(body.length, 24);
    dir.writeUInt16LE(n.length, 28);
    dir.writeUInt32LE(offset, 42);
    central.push(dir, n);
    offset += 30 + n.length + body.length;
  }
  const cd = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(central.length / 2, 8);
  end.writeUInt16LE(central.length / 2, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, cd, end]);
}

/** A one-paragraph Word document in large type. */
function docx(text) {
  const xml = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
  return zip({
    "[Content_Types].xml": `${xml}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>`,
    "_rels/.rels": `${xml}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>`,
    "word/document.xml": `${xml}<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:sz w:val="96"/></w:rPr><w:t>${text}</w:t></w:r></w:p></w:body></w:document>`,
  });
}

/** A PDF with one page per string, each in large type. */
function pdf(pages) {
  const objs = ["<< /Type /Catalog /Pages 2 0 R >>", ""];
  const kids = [];
  for (const text of pages) {
    const stream = `BT /F1 48 Tf 30 120 Td (${text}) Tj ET`;
    const page = objs.length + 1;
    kids.push(`${page} 0 R`);
    objs.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Contents ${page + 1} 0 R /Resources << /Font << /F1 FONT 0 R >> >> >>`);
    objs.push(`<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`);
  }
  objs[1] = `<< /Type /Pages /Kids [${kids.join(" ")}] /Count ${pages.length} >>`;
  objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
  const font = objs.length;
  let out = "%PDF-1.4\n";
  const at = [];
  objs.forEach((o, i) => {
    at.push(out.length);
    out += `${i + 1} 0 obj\n${o.replaceAll("FONT", String(font))}\nendobj\n`;
  });
  const xref = out.length;
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n${at.map((o) => `${String(o).padStart(10, "0")} 00000 n \n`).join("")}`;
  return out + `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
}

/** A PNG of one colour; zlib keeps a large one small on disk. */
function png(w, h, [r, g, b]) {
  const row = Buffer.alloc(1 + w * 3);
  for (let x = 0; x < w; x++) row.set([r, g, b], 1 + x * 3);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(zlib.crc32(td));
    return Buffer.concat([len, td, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

/** Half a second of a 440 Hz tone, 8 kHz mono 8-bit. */
function wav() {
  const n = 4000;
  const h = Buffer.alloc(44);
  h.write("RIFF", 0);
  h.writeUInt32LE(36 + n, 4);
  h.write("WAVEfmt ", 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(1, 22);
  h.writeUInt32LE(8000, 24);
  h.writeUInt32LE(8000, 28);
  h.writeUInt16LE(1, 32);
  h.writeUInt16LE(8, 34);
  h.write("data", 36);
  h.writeUInt32LE(n, 40);
  const body = Buffer.from(Array.from({ length: n }, (_, i) => 128 + Math.round(100 * Math.sin((2 * Math.PI * 440 * i) / 8000))));
  return Buffer.concat([h, body]);
}

/** The preview pane on, at a known width, for the length of `fn`. */
async function withPane(t, fn) {
  await t.call("setSetting", "detailsPane", false);
  await t.call("setSetting", "previewPane", true);
  await t.call("setSetting", "paneWidth", 360);
  try {
    await fn();
  } finally {
    await t.call("setSetting", "previewPane", false);
    await t.call("setSetting", "paneWidth", 300);
  }
}

async function showsText(t, name, expected) {
  await t.select([name]);
  await t.waitFor(`document.querySelector(${JSON.stringify(TEXT)})?.textContent === ${JSON.stringify(expected)}`, 5000, `${name} as text`);
}

async function showsMessage(t, name, start) {
  await t.select([name]);
  await t.waitFor(`document.querySelector(${JSON.stringify(MSG)})?.textContent.trim().startsWith(${JSON.stringify(start)})`, 5000, `${name}: "${start}"`);
}

const host = async (t) => JSON.parse(await t.ps("preview.ps1", "-ProcId", String(t.pid)));

/**
 * Wait for the native previewer to have drawn something, and return what
 * preview.ps1 saw. When another app's window covers the pane and the
 * previewer draws through WebView2, its pixels can't be read; that is noted,
 * not failed.
 */
async function nativeShown(t, what) {
  let last;
  try {
    await t.until(async () => {
      last = await host(t);
      return last.visible && last.owned && last.children.length > 0 && (last.ink > 0.002 || last.inkFrom === "covered");
    }, 30000);
  } catch {
    throw new Error(`${what} drawn by its previewer (last: ${JSON.stringify(last)})`);
  }
  if (last.inkFrom === "covered") console.log(`        note: another window covers the pane, so ${what}'s pixels weren't checked`);
  return last;
}

export default [
  {
    name: "text previews read UTF-8, UTF-16 with and without a byte-order mark, and the ANSI code page; binary files aren't shown as text",
    async run(t) {
      const d = t.dir("pv-text");
      t.mk(path.join(d, "utf8.txt"), "naïve café ✓");
      t.mk(path.join(d, "bom8.txt"), Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from("with a mark ✓")]));
      t.mk(path.join(d, "le.txt"), Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from("little endian ✓", "utf16le")]));
      t.mk(path.join(d, "be.txt"), Buffer.concat([Buffer.from([0xfe, 0xff]), Buffer.from("big endian ✓", "utf16le").swap16()]));
      t.mk(path.join(d, "nomark.log"), Buffer.from("no mark at all", "utf16le"));
      t.mk(path.join(d, "ansi.ini"), Buffer.from([0x63, 0x61, 0x66, 0xe9])); // "café" in Windows-1252
      t.mk(path.join(d, "LICENSE"), "MIT License\n");
      t.mk(path.join(d, "data.bin"), Buffer.from([0x4d, 0x5a, 0x90, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0xff, 0xff]));
      await t.nav(d);
      await withPane(t, async () => {
        await showsText(t, "utf8.txt", "naïve café ✓");
        await showsText(t, "bom8.txt", "with a mark ✓");
        await showsText(t, "le.txt", "little endian ✓");
        await showsText(t, "be.txt", "big endian ✓");
        await showsText(t, "nomark.log", "no mark at all");
        // This machine's ANSI code page decides; Western ones read 0xE9 as é.
        await t.select(["ansi.ini"]);
        await t.waitFor(`document.querySelector(${JSON.stringify(TEXT)})?.textContent.startsWith("caf")`, 5000, "ANSI text");
        await showsText(t, "LICENSE", "MIT License\n");
        await showsMessage(t, "data.bin", "No preview available.");
        t.ok(await t.ev(`!!document.querySelector(".preview .msg button.link")`), "an Open button for what can't be previewed");
      });
    },
  },
  {
    name: "a large text file shows its first 256 KB and says so, and a quick change of selection never shows the older file",
    async run(t) {
      const d = t.dir("pv-big");
      t.mk(path.join(d, "big.txt"), "x".repeat(300 * 1024));
      for (const n of ["a", "b", "c"]) t.mk(path.join(d, `${n}.txt`), `file ${n}`);
      await t.nav(d);
      await withPane(t, async () => {
        await t.select(["big.txt"]);
        await t.waitFor(`document.querySelector(${JSON.stringify(TEXT)})?.textContent.length === 262144`, 5000, "the first 256 KB");
        t.eq(await t.ev(`document.querySelector(".preview .more span")?.textContent`), "Showing the first 256 KB.", "and a note saying so");
        // Three selections without waiting: only the last one's preview may land.
        await t.ev(`(async () => { for (const n of ["a.txt", "b.txt", "c.txt"]) __e2e.selectNames([n]); })()`);
        await t.waitFor(`document.querySelector(${JSON.stringify(TEXT)})?.textContent === "file c"`, 5000, "c.txt's text");
        await t.sleep(400);
        t.eq(await t.ev(`document.querySelector(${JSON.stringify(TEXT)})?.textContent`), "file c", "still c.txt after the earlier reads finished");
      });
    },
  },
  {
    name: "an image fits the pane and a click shows it at actual size; a damaged image says there's no preview",
    async run(t) {
      const d = t.dir("pv-image");
      t.mk(path.join(d, "wide.png"), png(1600, 1000, [30, 120, 200]));
      t.mk(path.join(d, "broken.png"), "not a picture");
      await t.nav(d);
      await withPane(t, async () => {
        await t.select(["wide.png"]);
        await t.waitFor(`document.querySelector(".preview .media img")?.naturalWidth === 1600`, 5000, "the image loaded");
        const box = await t.ev(`(() => { const p = document.querySelector(".preview").getBoundingClientRect(); const i = document.querySelector(".preview .media img").getBoundingClientRect(); return { pw: p.width, iw: i.width, ih: i.height }; })()`);
        t.ok(box.iw <= box.pw && box.iw > 0, `fits the pane: ${JSON.stringify(box)}`);
        t.ok(Math.abs(box.iw / box.ih - 1.6) < 0.02, `keeps its shape: ${JSON.stringify(box)}`);
        await t.click(".preview .media img");
        await t.waitFor(`Math.round(document.querySelector(".preview .media.zoomed img")?.getBoundingClientRect().width ?? 0) === 1600`, 3000, "actual size");
        // At actual size the image's centre is out of view; click what's visible.
        await t.click(".preview");
        await t.waitFor(`!document.querySelector(".preview .media.zoomed")`, 3000, "back to fit");
        await showsMessage(t, "broken.png", "No preview available.");
      });
    },
  },
  {
    name: "audio and video play in the pane and seek, and a video the pane can't play shows Windows' thumbnail or says there's no preview",
    async run(t) {
      const d = t.dir("pv-media");
      t.mk(path.join(d, "tone.wav"), wav());
      t.mk(path.join(d, "broken.mp4"), "not a video");
      // A real H.264 video that Windows 11 ships with its setup screens, where present.
      const sample = "C:\\Windows\\SystemApps\\Microsoft.Windows.CloudExperienceHost_cw5n1h2txyewy\\media\\oobe-intro.mp4";
      const video = fs.existsSync(sample);
      if (video) fs.copyFileSync(sample, path.join(d, "clip.mp4"));
      await t.nav(d);
      await withPane(t, async () => {
        await t.select(["tone.wav"]);
        await t.waitFor(`(document.querySelector(".preview audio")?.readyState ?? 0) >= 1`, 5000, "the audio's metadata loaded");
        t.eq(await t.ev(`Math.round(document.querySelector(".preview audio").duration * 10) / 10`), 0.5, "half a second long");
        t.ok(await t.ev(`!!document.querySelector(".preview audio").controls`), "with controls");
        if (video) {
          await t.select(["clip.mp4"]);
          await t.waitFor(`(document.querySelector(".preview video")?.videoWidth ?? 0) > 0`, 10000, "the video's first frame");
          t.ok(await t.ev(`document.querySelector(".preview video").paused`), "it waits for Play");
          // Seeking reads from the middle of the file: the asset protocol must answer ranges.
          const seeked = await t.ev(`new Promise((done) => {
            const v = document.querySelector(".preview video");
            v.addEventListener("seeked", () => done(v.currentTime), { once: true });
            v.currentTime = v.duration / 2;
            setTimeout(() => done(-1), 5000);
          })`);
          t.ok(seeked > 0, `seeked to ${seeked}`);
        }
        await t.select(["broken.mp4"]);
        await t.waitFor(`!document.querySelector(".preview video") && (!!document.querySelector(".preview .media img") || !!document.querySelector(${JSON.stringify(MSG)}))`, 5000, "no broken player left");
      });
    },
  },
  {
    name: "a PDF shows in WebView2's viewer, in a frame of its own origin, and a file named .pdf that isn't one isn't opened as a page",
    async run(t) {
      const d = t.dir("pv-pdf");
      t.mk(path.join(d, "pages.pdf"), pdf(["Page one", "Page two"]));
      t.mk(path.join(d, "fake.pdf"), "<script>parent.postMessage('ran', '*')</script>");
      await t.nav(d);
      await withPane(t, async () => {
        await t.select(["pages.pdf"]);
        await t.waitFor(`document.querySelector(".preview iframe.pdf")?.src.startsWith("http://preview.localhost/")`, 5000, "the PDF's frame");
        // Another origin than the app's: the app's page can't reach into it, nor it out.
        await t.sleep(500);
        t.ok(await t.ev(`document.querySelector(".preview iframe.pdf").contentDocument === null`), "the frame is closed to the app's page");
        // And the viewer drew it: measured on screen where Filezy isn't covered.
        const r = await t.ev(`(() => { const s = devicePixelRatio; const b = document.querySelector(".preview iframe.pdf").getBoundingClientRect(); return [b.left, b.top, b.width, b.height].map((v) => Math.round(v * s)).join(","); })()`);
        let seen;
        try {
          await t.until(async () => {
            seen = JSON.parse(await t.ps("preview.ps1", "-ProcId", String(t.pid), "-Rect", r));
            return seen.inkFrom === "covered" || seen.ink > 0.002;
          }, 15000);
        } catch {
          throw new Error(`the PDF drawn (last: ${JSON.stringify(seen)})`);
        }
        if (seen.inkFrom === "covered") console.log("        note: another window covers the pane, so the PDF's pixels weren't checked");
        await t.ev(`window.__ran = false; window.addEventListener("message", (e) => e.data === "ran" && (window.__ran = true))`);
        await showsMessage(t, "fake.pdf", "No preview available.");
        await t.sleep(300);
        t.ok(!(await t.ev("window.__ran || !!document.querySelector('.preview iframe')")), "nothing of it ran");
      });
    },
  },
  {
    name: "Word files show in Windows' own previewer, out of process, hidden while a menu covers it and gone for the next file",
    async run(t) {
      const d = t.dir("pv-native");
      t.mk(path.join(d, "letter.docx"), docx("Filezy preview"));
      t.mk(path.join(d, "note.txt"), "plain");
      await t.nav(d);
      await withPane(t, async () => {
        const kind = await t.ev(`__e2e.invoke("preview_info", { path: ${JSON.stringify(path.join(d, "letter.docx"))} }).then((i) => i.kind)`);
        if (kind !== "native") t.skip("no Word previewer is registered here");
        await t.select(["letter.docx"]);
        const seen = await nativeShown(t, "letter.docx");
        t.ok(seen.children.every((c) => c.process.toLowerCase() !== "filezy"), `drawn by another process: ${JSON.stringify(seen.children)}`);
        // The list's menu, opened just left of the pane, reaches over it: the
        // previewer's window must step aside, then come back when it closes.
        const at = await t.ev(`(() => { const p = document.querySelector(".preview").getBoundingClientRect(); const s = document.querySelector(".scroller").getBoundingClientRect(); return { x: p.left - 24, y: s.bottom - 160 }; })()`);
        await t.click(at, { button: "right" });
        await t.waitFor(`(() => { const m = document.querySelector(".menu.ctxmenu")?.getBoundingClientRect(); const p = document.querySelector(".preview").getBoundingClientRect(); return !!m && m.right > p.left + 50; })()`, 3000, "the list's menu over the pane");
        await t.until(async () => !(await host(t)).visible, 3000, "hidden under the menu");
        await t.click(".statusbar");
        await t.waitFor(`!document.querySelector(".menu")`, 3000, "the menu closed");
        await t.until(async () => (await host(t)).visible, 3000, "back once the menu closed");
        await t.select(["note.txt"]);
        await t.waitFor(`document.querySelector(${JSON.stringify(TEXT)})?.textContent === "plain"`, 5000, "the text file's preview");
        await t.until(async () => !(await host(t)).visible, 3000, "the previewer's window is gone");
      });
    },
  },
  {
    name: "a file downloaded from the internet isn't handed to a previewer, and an online-only file isn't downloaded without asking",
    async run(t) {
      const d = t.dir("pv-guard");
      const doc = t.mk(path.join(d, "download.docx"), docx("From the internet"));
      fs.writeFileSync(`${doc}:Zone.Identifier`, "[ZoneTransfer]\r\nZoneId=3\r\n");
      const paper = t.mk(path.join(d, "download.pdf"), pdf(["From the internet"]));
      fs.writeFileSync(`${paper}:Zone.Identifier`, "[ZoneTransfer]\r\nZoneId=3\r\n");
      const cloud = t.mk(path.join(d, "cloud.txt"), "in the cloud");
      // Offline is what Windows' cloud files carry while their data is elsewhere.
      execFileSync("attrib.exe", ["+O", cloud], { windowsHide: true });
      await t.nav(d);
      try {
        await withPane(t, async () => {
          const kind = await t.ev(`__e2e.invoke("preview_info", { path: ${JSON.stringify(doc)} }).then((i) => i.kind)`);
          if (kind === "native") {
            await showsMessage(t, "download.docx", "The file you are attempting to preview could harm your computer.");
            t.eq((await host(t)).visible, false, "no previewer ran");
          }
          await showsMessage(t, "download.pdf", "The file you are attempting to preview could harm your computer.");
          t.ok(!(await t.ev("!!document.querySelector('.preview iframe')")), "the PDF wasn't opened");
          await showsMessage(t, "cloud.txt", "This file is available online only.");
          await t.click(".preview .msg button.link");
          await t.waitFor(`document.querySelector(${JSON.stringify(TEXT)})?.textContent === "in the cloud"`, 5000, "shown once asked");
        });
      } finally {
        execFileSync("attrib.exe", ["-O", cloud], { windowsHide: true });
      }
    },
  },
];
