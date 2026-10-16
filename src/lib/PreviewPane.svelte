<script lang="ts">
  // The preview pane. Text, images, audio, video and PDF are drawn here; other
  // files go to Windows' own preview handler (Word, Excel, fonts...), whose
  // window the backend keeps over this pane (src-tauri/src/preview.rs).
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { app, sel, rows, entryPath, openPath } from "./store.svelte";
  import { isArchive } from "./paths";

  type Kind = "text" | "image" | "thumb" | "audio" | "video" | "pdf" | "native" | "none";
  interface Info {
    kind: Kind;
    onlineOnly: boolean;
    untrusted: boolean;
  }
  type View =
    | { s: "empty" }
    | { s: "loading"; downloading: boolean }
    | { s: "failed"; why: string }
    | { s: "text"; text: string; truncated: boolean }
    | { s: "image" | "video" | "pdf"; src: string }
    | { s: "audio"; src: string; art: string | null }
    | { s: "thumb"; src: string }
    | { s: "native" }
    | { s: "online" }
    | { s: "untrusted" }
    | { s: "none" };

  /// Windows' own wording for a downloaded file it won't preview.
  const UNTRUSTED =
    "The file you are attempting to preview could harm your computer. If you trust the file and the source you received it from, open it to view its contents.";
  /// A handler waits this long for the selection to settle, so arrowing through
  /// a folder of documents doesn't start one for each.
  const NATIVE_DELAY_MS = 150;

  // First selected row (lowest index); undefined when nothing usable is selected.
  const current = $derived.by(() => {
    if (app.allSelected) return app.total ? rows[0] : undefined;
    let min = -1;
    for (const i of sel) if (min < 0 || i < min) min = i;
    return min < 0 ? undefined : rows[min];
  });
  const path = $derived(current && !current.is_dir ? entryPath(current) : null);
  /// What the preview depends on: a refresh that leaves the file as it was
  /// (a download only changes its attributes) doesn't load it again.
  const version = $derived(current && !current.is_dir ? `${current.modified}|${current.size}` : "");

  let view = $state<View>({ s: "empty" });
  /// A cloud file the user chose to download for its preview.
  let download = $state("");
  /// Bumped by Try again after a failed download.
  let attempt = $state(0);
  let seq = 0;

  $effect(() => {
    const p = path;
    const wanted = download;
    void version;
    void attempt;
    const my = ++seq;
    if (!p) view = { s: "empty" };
    else if (isArchive(app.path)) view = { s: "none" };
    else {
      view = { s: "loading", downloading: wanted === p };
      void load(p, my, wanted === p);
    }
  });

  async function load(p: string, my: number, downloadOk: boolean) {
    const stale = () => my !== seq;
    let why = "";
    const info = await invoke<Info>("preview_info", { path: p, download: downloadOk }).catch((e) => ((why = String(e)), null));
    if (stale()) return;
    // A download the user asked for says why it failed; anything else just has no preview.
    if (!info) return void (view = downloadOk && why ? { s: "failed", why } : { s: "none" });
    if (info.onlineOnly && !downloadOk && info.kind !== "none") return void (view = { s: "online" });
    if (info.untrusted) return void (view = { s: "untrusted" });
    switch (info.kind) {
      case "text": {
        const t = await invoke<{ text: string; truncated: boolean } | null>("preview_text", { path: p }).catch(() => null);
        if (!stale()) view = t ? { s: "text", ...t } : { s: "none" };
        return;
      }
      case "image":
      case "video":
      case "pdf":
        view = { s: info.kind, src: convertFileSrc(p) };
        return;
      case "audio": {
        view = { s: "audio", src: convertFileSrc(p), art: null };
        const art = await invoke<string | null>("thumb", { path: p, size: 256 }).catch(() => null);
        if (!stale() && view.s === "audio") view = { ...view, art };
        return;
      }
      case "thumb":
        return showThumb(p, my);
      case "native":
        view = { s: "native" };
        return;
      default:
        view = { s: "none" };
    }
  }

  /// Windows' thumbnail at the pane's size: images and videos the page can't decode.
  async function showThumb(p: string, my: number) {
    const size = Math.min(1024, Math.round((pane?.clientWidth ?? 320) * (window.devicePixelRatio || 1)));
    const src = await invoke<string | null>("thumb", { path: p, size }).catch(() => null);
    if (my === seq) view = src ? { s: "thumb", src } : { s: "none" };
  }

  /// A media file the page couldn't play after all: Windows' thumbnail, else nothing.
  function mediaFailed() {
    if (!path || (view.s !== "image" && view.s !== "video")) return;
    if (view.s === "video") void showThumb(path, seq);
    else view = { s: "none" };
  }

  // ---- image fit and zoom ----

  let pane = $state<HTMLElement | null>(null);
  let zoomed = $state(false);
  let natural = $state({ w: 0, h: 0 });
  const canZoom = $derived(!!pane && (natural.w > pane.clientWidth || natural.h > pane.clientHeight));
  $effect(() => {
    void path;
    zoomed = false;
    natural = { w: 0, h: 0 };
  });

  // ---- native handlers ----

  /// The pane's rectangle in physical pixels of the window, or null while a
  /// menu or dialog lies over it (the handler's window would hide it).
  function placeOf(box: HTMLElement) {
    const r = box.getBoundingClientRect();
    const covered = [...document.querySelectorAll<HTMLElement>(".menu, .scrim")].some((m) => {
      const o = m.getBoundingClientRect();
      return o.right > r.left && o.left < r.right && o.bottom > r.top && o.top < r.bottom;
    });
    if (covered || r.width < 1 || r.height < 1) return null;
    const s = window.devicePixelRatio || 1;
    const x = Math.round(r.left * s);
    const y = Math.round(r.top * s);
    return { x, y, w: Math.round(r.right * s) - x, h: Math.round(r.bottom * s) - y };
  }

  /// The pane's colours as COLORREFs (0x00BBGGRR), for handlers that follow them.
  function colorsOf(box: HTMLElement): [number, number] {
    const rgb = (c: string) => (c.match(/[\d.]+/g) ?? []).map(Number);
    const ref = ([r, g, b]: number[]) => (b << 16) | (g << 8) | r;
    const fg = rgb(getComputedStyle(box).color);
    // The first opaque background up the tree; Mica themes have none, so go by the text colour.
    for (let el: HTMLElement | null = box; el; el = el.parentElement) {
      const bg = rgb(getComputedStyle(el).backgroundColor);
      if (bg.length >= 3 && (bg[3] ?? 1) >= 0.99) return [ref(bg), ref(fg)];
    }
    const dark = fg[0] + fg[1] + fg[2] > 384;
    return [ref(dark ? [32, 32, 32] : [243, 243, 243]), ref(fg)];
  }

  /// `use:native={path}` on the box the handler draws over.
  function native(box: HTMLElement, file: string) {
    let sent = "";
    const report = () => {
      const place = placeOf(box);
      const key = JSON.stringify(place);
      if (key === sent) return;
      sent = key;
      invoke("preview_native_place", { place }).catch(() => {});
    };
    const resize = new ResizeObserver(report);
    resize.observe(box);
    // Menus and dialogs come and go anywhere in the document.
    const overlays = new MutationObserver(report);
    overlays.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", report);
    const my = seq;
    const timer = setTimeout(async () => {
      const place = placeOf(box);
      sent = JSON.stringify(place);
      // Covered right now: open at the pane's size, shown once the cover goes.
      const r = box.getBoundingClientRect();
      const s = window.devicePixelRatio || 1;
      const at = place ?? { x: Math.round(r.left * s), y: Math.round(r.top * s), w: Math.round(r.width * s), h: Math.round(r.height * s) };
      try {
        await invoke<boolean>("preview_native_open", { path: file, place: at, colors: colorsOf(box) });
        if (!place && my === seq) invoke("preview_native_place", { place: null }).catch(() => {});
      } catch {
        if (my === seq) view = { s: "none" };
      }
    }, NATIVE_DELAY_MS);
    return {
      destroy() {
        clearTimeout(timer);
        resize.disconnect();
        overlays.disconnect();
        window.removeEventListener("resize", report);
        invoke("preview_native_close").catch(() => {});
      },
    };
  }
</script>

<div class="preview" role="region" aria-label="Preview" bind:this={pane}>
  {#if view.s === "empty"}
    <div class="msg">Select a file to preview.</div>
  {:else if view.s === "loading"}
    <div class="msg loading">{view.downloading ? "Downloading…" : "Loading preview…"}</div>
  {:else if view.s === "failed"}
    <div class="msg">
      <p>{view.why}</p>
      <button class="link" onclick={() => attempt++}>Try again</button>
    </div>
  {:else if view.s === "text"}
    <pre class="text">{view.text}</pre>
    {#if view.truncated}
      <div class="more">
        <span>Showing the first 256 KB.</span>
        <button class="link" onclick={() => path && openPath(path)}>Open</button>
      </div>
    {/if}
  {:else if view.s === "image"}
    <div class="media" class:zoomed>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <img
        src={view.src}
        alt=""
        class:zoomable={canZoom}
        title={canZoom ? (zoomed ? "Fit to pane" : "Actual size") : undefined}
        onload={(ev) => {
          const img = ev.currentTarget as HTMLImageElement;
          natural = { w: img.naturalWidth, h: img.naturalHeight };
        }}
        onerror={mediaFailed}
        onclick={() => canZoom && (zoomed = !zoomed)}
      />
    </div>
  {:else if view.s === "thumb"}
    <div class="media"><img src={view.src} alt="" /></div>
  {:else if view.s === "video"}
    <div class="media">
      <!-- svelte-ignore a11y_media_has_caption -->
      <video src={view.src} controls preload="metadata" onerror={mediaFailed}></video>
    </div>
  {:else if view.s === "pdf"}
    <!-- WebView2's own PDF viewer, in a frame of another origin than the app. -->
    <iframe class="pdf" src="{view.src}#navpanes=0" title="PDF preview"></iframe>
  {:else if view.s === "audio"}
    <div class="media audio">
      {#if view.art}<img src={view.art} alt="" />{/if}
      <audio src={view.src} controls preload="metadata" onerror={() => (view = { s: "none" })}></audio>
    </div>
  {:else if view.s === "native" && path}
    {#key path}
      <div class="native" use:native={path}>
        <div class="msg loading">Loading preview…</div>
      </div>
    {/key}
  {:else if view.s === "online"}
    <div class="msg">
      <p>This file is available online only.</p>
      <button class="link" onclick={() => path && (download = path)}>Download and preview</button>
    </div>
  {:else if view.s === "untrusted"}
    <div class="msg">
      <p>{UNTRUSTED}</p>
      <button class="link" onclick={() => path && openPath(path)}>Open</button>
    </div>
  {:else}
    <div class="msg">
      <p>No preview available.</p>
      {#if path && !isArchive(app.path)}
        <button class="link" onclick={() => path && openPath(path)}>Open</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .preview {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .msg {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 16px;
    font-size: 13px;
    color: var(--text-3);
    text-align: center;
  }
  .msg p {
    margin: 0;
    max-width: 320px;
  }
  /* Appears only when loading takes long enough to notice. */
  .loading {
    animation: appear 0s linear 300ms both;
  }
  @keyframes appear {
    from {
      visibility: hidden;
    }
    to {
      visibility: visible;
    }
  }
  .link {
    padding: 2px 6px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--accent);
    font-size: 13px;
  }
  .link:hover {
    background: var(--surface-hover);
  }
  .link:focus-visible {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }
  .text {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 12px;
    font: 12px/1.5 Consolas, monospace;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }
  .more {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px 6px 12px;
    border-top: 1px solid var(--stroke);
    font-size: 12px;
    color: var(--text-2);
  }
  .media {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 16px;
  }
  .media img,
  .media video {
    max-width: 100%;
    max-height: 100%;
    min-height: 0;
    object-fit: contain;
  }
  .media .zoomable {
    cursor: zoom-in;
  }
  /* Actual size: scroll around an image larger than the pane. */
  .media.zoomed {
    display: block;
    overflow: auto;
  }
  .media.zoomed img {
    max-width: none;
    max-height: none;
    cursor: zoom-out;
  }
  .audio img {
    max-height: calc(100% - 80px);
  }
  .audio audio {
    width: 100%;
    flex-shrink: 0;
  }
  .native {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .pdf {
    flex: 1;
    min-height: 0;
    border: 0;
  }
</style>
