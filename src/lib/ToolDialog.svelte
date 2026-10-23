<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { tool, sizes, ALGOS, runHashes, stopTool, closeTool, onProgress, onSize, analyzeFolder, type Size } from "./tools.svelte";
  import { navigate, openPath, fmtSizeAuto as fmtBytes } from "./store.svelte";
  import { baseName } from "./paths";

  onMount(() => {
    const un = [listen<Parameters<typeof onProgress>[0]>("tool-progress", (e) => onProgress(e.payload)), listen<Size>("tool-size", (e) => onSize(e.payload))];
    return () => un.forEach((u) => u.then((f) => f()));
  });

  let closeBtn: HTMLButtonElement | undefined = $state();
  let returnFocus: Element | null = null;
  $effect(() => {
    if (!tool.kind) return;
    returnFocus = document.activeElement;
    queueMicrotask(() => closeBtn?.focus());
  });
  function close() {
    closeTool();
    (returnFocus as HTMLElement | null)?.focus?.();
  }

  // Checksums: a pasted hash is matched against every result.
  let expected = $state("");
  const want = $derived(expected.trim().toLowerCase().replace(/\s+/g, ""));
  const matches = $derived(want ? tool.hashes.flatMap((h) => h.hashes.filter(([, v]) => v === want).map(([a]) => `${baseName(h.path)} (${a})`)) : []);
  let copied = $state("");
  function copy(v: string) {
    navigator.clipboard.writeText(v).then(() => {
      copied = v;
      setTimeout(() => copied === v && (copied = ""), 1500);
    }).catch((e) => (tool.error = `Couldn't copy. ${e}`));
  }
  function toggleAlgo(a: string) {
    const on = tool.algos.includes(a);
    if (on && tool.algos.length === 1) return; // at least one algorithm
    tool.algos = on ? tool.algos.filter((x) => x !== a) : ALGOS.filter((x) => x === a || tool.algos.includes(x));
    runHashes();
  }

  // Sizes, largest first; the bar is relative to the largest. Long lists show
  // their largest items until "Show all".
  const SHOWN = 200;
  let showAll = $state(false);
  $effect(() => {
    void tool.job;
    showAll = false;
  });
  const shown = $derived(showAll ? sizes.list : sizes.list.slice(0, SHOWN));
  const largest = $derived(Math.max(1, sizes.list[0]?.bytes ?? 0));
  const sum = $derived(
    sizes.list.reduce((t, s) => ({ bytes: t.bytes + s.bytes, files: t.files + s.files, folders: t.folders + s.folders + (tool.dir && s.is_dir ? 1 : 0), skipped: t.skipped + s.skipped, links: t.links + s.links }), { bytes: 0, files: 0, folders: 0, skipped: 0, links: 0 }),
  );
  function openItem(s: Size) {
    if (s.is_dir) {
      close();
      navigate(s.path);
    } else openPath(s.path);
  }

  const pct = $derived(tool.total ? Math.min(100, (tool.done / tool.total) * 100) : 0);

  function onKeydown(ev: KeyboardEvent) {
    if (!tool.kind) return;
    ev.stopPropagation();
    if (ev.key === "Escape") {
      ev.preventDefault();
      close();
    }
  }
</script>

<svelte:window onkeydowncapture={onKeydown} />

{#if tool.kind}
  <div class="scrim" role="presentation">
    <div class="dlg" role="dialog" aria-modal="true" aria-labelledby="tool-title" aria-busy={tool.busy}>
      <h2 id="tool-title">
        {msg(tool.title)}
        {#if tool.dir}<span class="sub" title={tool.dir}>{baseName(tool.dir) || tool.dir}</span>{/if}
      </h2>

      {#if tool.kind === "hash"}
        <div class="algos" role="group" aria-label={msg("Algorithms")}>
          {#each ALGOS as a (a)}
            <label><input type="checkbox" checked={tool.algos.includes(a)} disabled={tool.algos.length === 1 && tool.algos.includes(a)} onchange={() => toggleAlgo(a)} /> {a}</label>
          {/each}
        </div>
        <div class="list">
          {#each tool.paths as p (p)}
            {@const r = tool.hashes.find((h) => h.path === p)}
            <div class="file">
              <div class="fname" title={p}>{baseName(p)} {#if r}<span class="dim">{fmtBytes(r.size)}</span>{/if}</div>
              {#if r?.error}
                <div class="err">{r.error}</div>
              {:else if r}
                {#each r.hashes as [a, v] (a)}
                  <div class="hash" class:match={!!want && v === want}>
                    <span class="alg">{a}</span>
                    <code>{v}</code>
                    <button class="mini" onclick={() => copy(v)} aria-label={`${msg("Copy")} ${a}`} title={msg("Copy")}>
                      <span class="glyph">{copied === v ? "" : ""}</span>
                    </button>
                  </div>
                {/each}
              {:else}
                <div class="dim">{tool.busy ? msg("Calculating…") : msg("Stopped.")}</div>
              {/if}
            </div>
          {/each}
        </div>
        <label class="field">
          <span>{msg("Compare with a published checksum")}</span>
          <input bind:value={expected} spellcheck="false" placeholder={msg("Paste a checksum")} />
        </label>
        {#if want && !tool.busy}
          <p class="verdict" class:ok={matches.length > 0} role="status">
            {matches.length ? `${msg("Matches")} ${matches.join(", ")}` : msg("No file matches this checksum.")}
          </p>
        {/if}
      {:else if tool.kind === "compare"}
        <div class="list">
          {#each tool.paths as p, i (p)}
            <div class="file"><div class="fname" title={p}>{baseName(p)}</div><div class="dim">{p}</div>
              {#if tool.compared}<div class="dim">{fmtBytes(i ? tool.compared.size_b : tool.compared.size_a)}</div>{/if}</div>
          {/each}
        </div>
        {#if tool.compared}
          <p class="verdict" class:ok={tool.compared.same} role="status">
            {#if tool.compared.same}
              {msg("The files are identical, byte for byte.")}
            {:else if tool.compared.size_a !== tool.compared.size_b && tool.compared.first_diff === Math.min(tool.compared.size_a, tool.compared.size_b)}
              {msg("The files differ: one is longer, and the shorter one matches its start.")}
            {:else}
              {msg("The files differ, first at byte")} {tool.compared.first_diff?.toLocaleString()}.
            {/if}
          </p>
        {/if}
      {:else}
        <div class="list sizes" role="list">
          {#each shown as s (s.path)}
            <div class="srow" role="listitem" ondblclick={() => openItem(s)}>
              <span class="glyph kind" aria-hidden="true">{s.is_dir ? "" : ""}</span>
              <span class="sname" title={s.path}>{baseName(s.path)}</span>
              <span class="bar" aria-hidden="true"><span style="width:{(s.bytes / largest) * 100}%"></span></span>
              <span class="ssize">{fmtBytes(s.bytes)}{s.done ? "" : "…"}</span>
              <span class="scount dim">{s.is_dir ? `${s.files.toLocaleString()} ${msg("files")}` : ""}</span>
            </div>
          {:else}
            <div class="dim pad">{tool.busy ? msg("Measuring…") : msg("This folder is empty.")}</div>
          {/each}
          {#if shown.length < sizes.list.length}
            <button class="btn more" onclick={() => (showAll = true)}>{msg("Show all")} ({sizes.list.length.toLocaleString()})</button>
          {/if}
        </div>
        <p class="verdict" role="status">
          {fmtBytes(sum.bytes)} {msg("in")} {sum.files.toLocaleString()} {msg("files and")} {sum.folders.toLocaleString()} {msg("folders")}{tool.busy ? "…" : "."}
          {#if sum.skipped}<span class="warn">{sum.skipped} {sum.skipped === 1 ? msg("folder couldn't be read") : msg("folders couldn't be read")}.</span>{/if}
          {#if sum.links}<span class="dim">{sum.links} {sum.links === 1 ? msg("link not followed") : msg("links not followed")}.</span>{/if}
        </p>
      {/if}

      {#if tool.error}<p class="verdict err" role="alert">{tool.error}</p>{/if}

      <div class="btns">
        {#if tool.busy}
          <div class="progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(pct)}>
            <span style="width:{tool.total ? pct : 30}%" class:indet={!tool.total}></span>
          </div>
          <button class="btn" onclick={stopTool}>{msg("Stop")}</button>
        {:else if tool.kind === "sizes" && !tool.dir}
          <button class="btn" onclick={() => { closeTool(); analyzeFolder(); }}>{msg("Whole folder")}</button>
        {/if}
        <button class="btn primary" bind:this={closeBtn} onclick={close}>{msg("Close")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 2000;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.3);
  }
  .dlg {
    width: min(640px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    display: flex;
    flex-direction: column;
    background: var(--menu-bg);
    border: 1px solid var(--stroke-strong);
    border-radius: 8px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.4);
    padding: 20px 24px 0;
    overflow: hidden;
  }
  h2 {
    font-size: 18px;
    font-weight: 600;
    margin-bottom: 12px;
    display: flex;
    gap: 8px;
    align-items: baseline;
    min-width: 0;
  }
  .sub {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .algos {
    display: flex;
    gap: 18px;
    font-size: 13px;
    margin-bottom: 10px;
    color: var(--text-2);
  }
  .algos label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  input[type="checkbox"] {
    accent-color: var(--accent);
  }
  .list {
    flex: 1;
    min-height: 60px;
    max-height: 340px;
    overflow-y: auto;
    border: 1px solid var(--stroke);
    border-radius: 4px;
    padding: 6px 8px;
  }
  .file + .file {
    border-top: 1px solid var(--stroke);
    margin-top: 6px;
    padding-top: 6px;
  }
  .fname {
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dim {
    color: var(--text-3);
    font-size: 12px;
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pad {
    padding: 8px 4px;
  }
  .more {
    display: block;
    margin: 6px auto 2px;
  }
  .hash {
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    font-size: 12px;
    border-radius: 4px;
    padding: 0 4px;
  }
  .hash.match {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .alg {
    color: var(--text-3);
  }
  code {
    font-family: Consolas, "Cascadia Mono", monospace;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .mini {
    width: 26px;
    height: 24px;
    border-radius: 4px;
    display: grid;
    place-items: center;
  }
  .mini:hover {
    background: var(--hover);
  }
  .mini .glyph {
    font-size: 12px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
    font-size: 13px;
    color: var(--text-2);
  }
  .field input {
    height: 32px;
    padding: 0 10px;
    background: var(--input-bg);
    border: 1px solid var(--stroke);
    border-bottom: 2px solid var(--accent);
    border-radius: 4px;
    font-family: Consolas, "Cascadia Mono", monospace;
    user-select: text;
  }
  .verdict {
    margin-top: 10px;
    font-size: 13px;
    color: var(--text-2);
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
  }
  .verdict.ok {
    color: var(--text);
    font-weight: 600;
  }
  .err,
  .verdict.err,
  .warn {
    color: #ff99a4;
  }
  .srow {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) minmax(60px, 160px) 80px 90px;
    align-items: center;
    gap: 8px;
    height: 28px;
    font-size: 13px;
    padding: 0 4px;
    border-radius: 4px;
  }
  .srow:hover {
    background: var(--hover);
  }
  .kind {
    font-size: 14px;
    color: var(--text-3);
  }
  .sname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    height: 8px;
    border-radius: 4px;
    background: var(--stroke);
    overflow: hidden;
  }
  .bar span,
  .progress span {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: inherit;
  }
  .ssize {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .scount {
    text-align: right;
  }
  .btns {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: flex-end;
    margin: 16px -24px 0;
    padding: 16px 24px;
    background: var(--surface);
    border-top: 1px solid var(--stroke);
  }
  .progress {
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: var(--stroke);
    overflow: hidden;
  }
  .progress .indet {
    animation: slide 1.2s ease-in-out infinite;
  }
  @keyframes slide {
    from { transform: translateX(-100%); }
    to { transform: translateX(340%); }
  }
  @media (prefers-reduced-motion: reduce) {
    .progress .indet { animation: none; }
  }
  .btn {
    min-width: 96px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
    background: var(--input-bg);
  }
  .btn:hover {
    background: var(--surface-hover);
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--menu-bg);
  }
  @media (max-width: 520px) {
    .srow { grid-template-columns: 20px minmax(0, 1fr) 70px; }
    .bar, .scount { display: none; }
  }
</style>
