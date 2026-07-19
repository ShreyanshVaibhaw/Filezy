<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { SvelteMap } from "svelte/reactivity";
  import { app, navigate, recentFiles, fmtDate, folderName, shellIcon, type ExtMeta } from "./store.svelte";

  const c = String.fromCharCode;
  const glyphs: Record<string, string> = {
    Desktop: c(0xe7f4),
    Documents: c(0xe8a5),
    Downloads: c(0xe896),
    Pictures: c(0xeb9f),
    Music: c(0xe8d6),
    Videos: c(0xe714),
  };

  // Per-extension icons for the Recent list, fetched once each.
  const icons = new SvelteMap<string, ExtMeta>();
  function iconFor(ext: string): string | null {
    if (!icons.has(ext)) {
      icons.set(ext, { icon: null, type_name: "" });
      invoke<ExtMeta>("ext_meta", { ext, isDir: false }).then((m) => icons.set(ext, m));
    }
    return icons.get(ext)?.icon ?? null;
  }

  function openFile(path: string) {
    invoke("open_path", { path }).catch((e) => (app.status = String(e)));
  }
  function parentDir(p: string): string {
    const i = p.replace(/\\$/, "").lastIndexOf("\\");
    return i <= 2 ? p.slice(0, 3) : p.slice(0, i);
  }
</script>

<div class="homepane">
  <div class="section">Quick access</div>
  <div class="qtiles">
    {#each app.specials as [name, path] (path)}
      <button class="qtile" ondblclick={() => navigate(path)} title={path}>
        {#if shellIcon(path, 40)}
          <img class="qimg" src={shellIcon(path, 40)} alt="" width="40" height="40" />
        {:else}
          <span class="glyph qicon">{glyphs[name] ?? c(0xe8b7)}</span>
        {/if}
        <span class="qinfo">
          <span class="qname">{name}</span>
          <span class="qsub">Stored locally</span>
        </span>
        <span class="glyph pin">&#xE840;</span>
      </button>
    {/each}
    {#each app.pins as p (p)}
      <button class="qtile" ondblclick={() => navigate(p)} title={p}>
        {#if shellIcon(p, 40)}
          <img class="qimg" src={shellIcon(p, 40)} alt="" width="40" height="40" />
        {:else}
          <span class="glyph qicon">{c(0xe8b7)}</span>
        {/if}
        <span class="qinfo">
          <span class="qname">{folderName(p)}</span>
          <span class="qsub">{parentDir(p)}</span>
        </span>
        <span class="glyph pin">&#xE840;</span>
      </button>
    {/each}
  </div>

  <div class="section">Recent files</div>
  {#if !recentFiles.list.length}
    <div class="empty">After you open some files, they show up here.</div>
  {:else}
    <div class="recent">
      {#each recentFiles.list as f (f.path)}
        <button class="rrow" ondblclick={() => openFile(f.path)} title={f.path}>
          {#if iconFor(f.ext)}
            <img class="ricon" src={iconFor(f.ext)} alt="" width="16" height="16" />
          {:else}
            <span class="glyph ricon">&#xE7C3;</span>
          {/if}
          <span class="rname">{f.name}</span>
          <span class="rpath">{parentDir(f.path)}</span>
          <span class="rdate">{fmtDate(f.modified)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .homepane {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
  }
  .section {
    font-size: 13px;
    color: var(--text-2);
    padding: 6px 0 10px 4px;
  }
  .qtiles {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 14px;
  }
  .qtile {
    width: 216px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 4px;
    text-align: left;
  }
  .qtile:hover {
    background: var(--hover);
  }
  .qicon {
    font-size: 28px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .qimg {
    flex-shrink: 0;
  }
  .qinfo {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .qname {
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .qsub {
    font-size: 11px;
    color: var(--text-3);
  }
  .pin {
    font-size: 11px;
    color: var(--text-3);
    transform: rotate(45deg);
  }
  .empty {
    font-size: 13px;
    color: var(--text-3);
    padding: 8px 4px;
  }
  .recent {
    display: flex;
    flex-direction: column;
  }
  .rrow {
    height: 34px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border-radius: 4px;
    text-align: left;
  }
  .rrow:hover {
    background: var(--hover);
  }
  .ricon {
    flex-shrink: 0;
    font-size: 15px;
    color: var(--text-2);
  }
  .rname {
    flex: 0 1 auto;
    min-width: 0;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rpath {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-left: 12px;
  }
  .rdate {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-3);
  }
</style>
