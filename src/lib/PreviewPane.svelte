<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    app, sel, rows, entryPath, requestThumb, thumbOf, requestMeta, typeOf,
    fmtDate, fmtSizeAuto, selCount,
  } from "./store.svelte";

  const TEXT_EXTS = new Set([
    "txt", "md", "log", "json", "xml", "csv", "ini", "toml", "yaml", "yml",
    "js", "ts", "rs", "py", "html", "css", "svelte", "bat", "ps1", "sh", "c", "h", "cpp",
  ]);

  // First selected row (lowest index); undefined when nothing usable is selected.
  const current = $derived.by(() => {
    if (app.allSelected) return app.total ? { i: 0, e: rows[0] } : undefined;
    let min = -1;
    for (const i of sel) if (min < 0 || i < min) min = i;
    return min < 0 ? undefined : { i: min, e: rows[min] };
  });

  let text = $state<string | null>(null);
  $effect(() => {
    const c = current;
    text = null;
    if (!c?.e) return;
    requestMeta(c.e);
    if (!c.e.is_dir && TEXT_EXTS.has(c.e.ext)) {
      invoke<string>("read_text", { path: entryPath(c.e) })
        .then((t) => (text = t))
        .catch(() => {});
    } else {
      requestThumb(c.i, c.e, 256);
    }
  });
</script>

<div class="preview">
  {#if !current?.e}
    <div class="empty">Select a file to preview.</div>
  {:else}
    {@const e = current.e}
    {#if text !== null}
      <pre class="text">{text}</pre>
    {:else}
      <div class="thumbwrap">
        {#if thumbOf(e, 256)}
          <img src={thumbOf(e, 256)} alt="" />
        {/if}
      </div>
    {/if}
    <div class="meta">
      <div class="fname">{e.name}</div>
      <div class="frow">{typeOf(e)}</div>
      {#if !e.is_dir}
        <div class="frow">{fmtSizeAuto(e.size)}</div>
      {/if}
      <div class="frow">{fmtDate(e.modified)}</div>
      {#if selCount() > 1}
        <div class="frow">{selCount().toLocaleString()} items selected</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .preview {
    width: 300px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--stroke);
    overflow: hidden;
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    font-size: 13px;
    color: var(--text-3);
    padding: 16px;
    text-align: center;
  }
  .thumbwrap {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
    padding: 16px;
  }
  .thumbwrap img {
    max-width: 100%;
    max-height: 100%;
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
  .meta {
    flex-shrink: 0;
    padding: 12px 16px 16px;
    border-top: 1px solid var(--stroke);
  }
  .fname {
    font-size: 13px;
    font-weight: 600;
    word-break: break-all;
    margin-bottom: 4px;
  }
  .frow {
    font-size: 12px;
    color: var(--text-2);
    padding: 1px 0;
  }
  @media (max-width: 900px) {
    .preview {
      display: none;
    }
  }
</style>
