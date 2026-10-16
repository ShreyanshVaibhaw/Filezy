<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { batch, batchNames, commitBatch } from "./ops.svelte";
  import { baseName, joinPath, parentPath } from "./paths";

  let base = $state("");
  let input: HTMLInputElement | undefined = $state();
  // New names that already belong to an item outside the batch.
  let taken = $state(new Set<string>());

  $effect(() => {
    if (!batch.open) return;
    // Start from the first item's name without its extension, selected.
    const first = batch.items[0];
    const n = baseName(first.path);
    const dot = first.is_dir ? -1 : n.lastIndexOf(".");
    base = dot > 0 ? n.slice(0, dot) : n;
    queueMicrotask(() => {
      input?.focus();
      input?.select();
    });
  });

  const names = $derived(batchNames(base, batch.items));
  const targets = $derived(batch.items.map((it, i) => joinPath(parentPath(it.path) ?? "", names[i])));
  const invalid = $derived(!base.trim() || /[\\/:*?"<>|]/.test(base) ? "A file name can't contain any of the following characters: \\ / : * ? \" < > |" : "");
  const clashes = $derived(targets.filter((t) => taken.has(t.toLowerCase())).length);

  $effect(() => {
    if (!batch.open) return;
    const ts = targets;
    const sources = new Set(batch.items.map((it) => it.path.toLowerCase()));
    const timer = setTimeout(async () => {
      const st = await invoke<{ exists: boolean }[]>("path_stamps", { paths: ts }).catch(() => []);
      taken = new Set(ts.filter((p, k) => st[k]?.exists && !sources.has(p.toLowerCase())).map((p) => p.toLowerCase()));
    }, 120);
    return () => clearTimeout(timer);
  });

  function go() {
    if (!invalid) commitBatch(base);
  }

  function onKeydown(ev: KeyboardEvent) {
    if (!batch.open) return;
    // The dialog owns the keyboard while open.
    ev.stopPropagation();
    if (ev.key === "Escape") {
      ev.preventDefault();
      batch.open = false;
    } else if (ev.key === "Enter" && ev.target === input) {
      ev.preventDefault();
      go();
    }
  }
</script>

<svelte:window onkeydowncapture={onKeydown} />

{#if batch.open}
  <div class="scrim" role="presentation">
    <div class="dlg" role="dialog" aria-modal="true" aria-labelledby="br-title">
      <h2 id="br-title">Rename {batch.items.length} items</h2>
      <label class="field">
        <span>New name</span>
        <input bind:this={input} bind:value={base} spellcheck="false" aria-describedby="br-hint" />
      </label>
      <p id="br-hint" class="hint" class:err={!!invalid}>
        {invalid || "Each item gets this name and a number, in list order, and keeps its own extension."}
      </p>
      <div class="preview" role="list" aria-label="Preview">
        {#each batch.items as it, i (it.path)}
          {@const bad = taken.has(targets[i].toLowerCase())}
          <div class="prow" class:bad role="listitem">
            <span class="old" title={baseName(it.path)}>{baseName(it.path)}</span>
            <span class="glyph arrow" aria-hidden="true">&#xE72A;</span>
            <span class="new" title={names[i]}>{names[i]}</span>
            {#if bad}<span class="why">name taken</span>{/if}
          </div>
        {/each}
      </div>
      {#if clashes}
        <p class="hint err">
          {clashes === 1 ? "1 new name is" : `${clashes} new names are`} already used in that folder; those items keep their names.
        </p>
      {/if}
      <div class="btns">
        <button class="btn primary" disabled={!!invalid} onclick={go}>Rename</button>
        <button class="btn" onclick={() => (batch.open = false)}>Cancel</button>
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
    width: min(560px, calc(100vw - 32px));
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
    margin-bottom: 14px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
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
    user-select: text;
  }
  .hint {
    font-size: 12px;
    color: var(--text-3);
    margin: 8px 0 10px;
  }
  .hint.err {
    color: #ff99a4;
  }
  .preview {
    flex: 1;
    min-height: 60px;
    max-height: 280px;
    overflow-y: auto;
    border: 1px solid var(--stroke);
    border-radius: 4px;
    padding: 4px;
  }
  .prow {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 8px;
    font-size: 13px;
  }
  .prow.bad .new {
    color: #ff99a4;
    text-decoration: line-through;
  }
  .old {
    color: var(--text-3);
  }
  .old,
  .new {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .arrow {
    font-size: 10px;
    color: var(--text-3);
  }
  .why {
    font-size: 11px;
    color: #ff99a4;
  }
  .btns {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin: 16px -24px 0;
    padding: 16px 24px;
    background: var(--surface);
    border-top: 1px solid var(--stroke);
  }
  .btn {
    min-width: 96px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--stroke-strong);
    border-radius: 4px;
    background: var(--input-bg);
  }
  .btn:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--menu-bg);
  }
  .btn.primary:disabled {
    opacity: 0.5;
  }
</style>
