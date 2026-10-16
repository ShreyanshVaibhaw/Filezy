<!-- Explorer's Details pane: what the selection is, its dates and sizes, a
     shortcut's target, and the document properties its files carry. Properties
     the file's handler can write are edited in place; for several items each
     one reports whether the change took. -->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    app, rows, sel, entryPath, requestMeta, requestThumb, thumbOf, typeOf, fmtDate, fmtSizeAuto,
    selCount, selRef, folderName, refresh, withSel, requestStatus, statusOf,
  } from "./store.svelte";
  import type { Entry } from "./store.svelte";
  import { baseName, isArchive } from "./paths";

  /** The properties shown, in Explorer's order, with their labels. */
  const PROPS: [string, string][] = [
    ["title", "Title"],
    ["author", "Authors"],
    ["tags", "Tags"],
    ["category", "Categories"],
    ["comment", "Comments"],
    ["dimensions", "Dimensions"],
    ["duration", "Length"],
  ];
  const PLACEHOLDER: Record<string, string> = {
    title: "Add a title",
    author: "Add an author",
    tags: "Add a tag",
    category: "Add a category",
    comment: "Add comments",
  };
  /** Up to this many selected rows are read for shared values. */
  const SAMPLE = 50;

  /** The selected rows the page has (lowest index first), at most SAMPLE. */
  const picked = $derived.by((): Entry[] => {
    const out: Entry[] = [];
    if (app.allSelected) {
      for (let i = 0; i < app.total && out.length < SAMPLE; i++) if (rows[i]) out.push(rows[i]!);
    } else {
      for (const i of [...sel].sort((a, b) => a - b).slice(0, SAMPLE)) if (rows[i]) out.push(rows[i]!);
    }
    return out;
  });
  const count = $derived(selCount());
  const one = $derived(count === 1 ? picked[0] : undefined);

  let values = $state<Record<string, string>>({});
  let writable = $state<string[]>([]);
  let target = $state<string | null>(null);
  let stats = $state<{ bytes: number; files: number; oldest: number; newest: number } | null>(null);
  let seq = 0;
  /** The items shown: a refresh of the same ones keeps their values until new ones arrive. */
  let shown = "";

  // Read the properties (shared values for several items, "(multiple values)"
  // where they differ), what can be written, a shortcut's target and the totals.
  $effect(() => {
    const items = picked;
    const n = count;
    void app.rev;
    const s = ++seq;
    const key = `${n}|${items.map((e) => entryPath(e)).join("|")}`;
    if (key !== shown) {
      shown = key;
      values = {};
      writable = [];
      target = null;
      stats = null;
    }
    if (!items.length) return;
    for (const e of items.slice(0, 1)) {
      requestMeta(e);
      requestThumb(e, 96);
      requestStatus(e);
    }
    // An archive's entries have only what the archive lists: name, size, date.
    const files = isArchive(app.path) ? [] : items.filter((e) => !e.is_dir);
    const keys = PROPS.map(([k]) => k);
    if (files.length) {
      invoke<(string | null)[][]>("props_for", { items: files.map((e) => [entryPath(e), e.modified]), keys })
        .then((vals) => {
          if (s !== seq) return;
          const out: Record<string, string> = {};
          keys.forEach((k, j) => {
            const set = new Set(vals.map((v) => v[j] ?? ""));
            const v = set.size === 1 ? [...set][0] : "(multiple values)";
            if (v) out[k] = v;
          });
          values = out;
        })
        .catch(() => {});
      // Editable where any of the first few files can store the property; a
      // write then says which of the selected files couldn't.
      Promise.all(files.slice(0, 5).map((e) => invoke<string[]>("props_writable", { path: entryPath(e) }).catch(() => [])))
        .then((ws) => s === seq && (writable = [...new Set(ws.flat())]));
    }
    if (n === 1 && items[0].ext === "lnk" && !isArchive(app.path)) {
      invoke<string | null>("link_target", { path: entryPath(items[0]) }).then((t) => s === seq && (target = t));
    }
    if (n > 1) {
      withSel((ref) => invoke<{ bytes: number; files: number; oldest: number; newest: number }>("selection_stats", { sel: ref }))
        .then((st) => s === seq && (stats = st))
        .catch(() => {});
    }
  });

  // ---- editing ----
  let editing = $state("");
  let draft = $state("");
  let saving = false;

  function edit(key: string) {
    editing = key;
    draft = values[key] === "(multiple values)" ? "" : (values[key] ?? "");
  }

  async function save() {
    const key = editing;
    editing = "";
    if (!key || saving || draft === (values[key] ?? "")) return;
    saving = true;
    const label = PROPS.find(([k]) => k === key)?.[1] ?? key;
    try {
      const res = await invoke<{ path: string; error: string | null }[]>("props_write", { sel: selRef(), key, value: draft });
      const failed = res.filter((r) => r.error);
      if (failed.length) {
        const why = `${baseName(failed[0].path)}: ${failed[0].error}`;
        app.status =
          res.length === 1 ? `${label} wasn't saved. ${why}` : `${label} saved for ${res.length - failed.length} of ${res.length} items. ${why}`;
      }
    } catch (e) {
      app.status = String(e);
    } finally {
      saving = false;
    }
    // Writing changed the files' modified times: list them again, which reads them anew.
    await refresh();
  }

  function onKey(ev: KeyboardEvent) {
    ev.stopPropagation();
    if (ev.key === "Enter") save();
    else if (ev.key === "Escape") editing = "";
  }

  function focus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<div class="details" aria-label="Details pane">
  {#if !count}
    <div class="head">
      <div class="big glyph" aria-hidden="true">&#xE8B7;</div>
      <div class="name">{folderName(app.search?.root ?? app.path)}</div>
      <div class="sub">{app.total.toLocaleString()} {app.total === 1 ? "item" : "items"}</div>
    </div>
  {:else}
    <div class="head">
      {#if one}
        {#if thumbOf(one, 96)}
          <img class="big" src={thumbOf(one, 96)} alt="" />
        {:else}
          <div class="big glyph" aria-hidden="true">{one.is_dir ? "" : ""}</div>
        {/if}
        <div class="name" title={one.name}>{one.name}</div>
        <div class="sub">{typeOf(one)}</div>
      {:else}
        <div class="big glyph" aria-hidden="true">&#xE8A9;</div>
        <div class="name">{count.toLocaleString()} items selected</div>
      {/if}
    </div>
    <dl class="fields">
      {#if one}
        {#if app.search}
          <dt>Location</dt>
          <dd title={one.parent ?? app.path}>{one.parent ?? app.path}</dd>
        {/if}
        {#if !one.is_dir}
          <dt>Size</dt>
          <dd>{fmtSizeAuto(one.size)}</dd>
        {/if}
        <dt>Date modified</dt>
        <dd>{fmtDate(one.modified)}</dd>
        <dt>Date created</dt>
        <dd>{fmtDate(one.created)}</dd>
        {#if statusOf(one)}
          <dt>Availability status</dt>
          <dd>{statusOf(one)}</dd>
        {/if}
        {#if target}
          <dt>Target</dt>
          <dd class="wrap" title={target}>{target}</dd>
        {/if}
      {:else if stats}
        {#if stats.files}
          <dt>Total size</dt>
          <dd>{fmtSizeAuto(stats.bytes)}</dd>
        {/if}
        {#if stats.oldest}
          <dt>Date modified</dt>
          <dd>{stats.oldest === stats.newest ? fmtDate(stats.newest) : `${fmtDate(stats.oldest)} - ${fmtDate(stats.newest)}`}</dd>
        {/if}
      {/if}
      {#each PROPS as [key, label] (key)}
        {@const canEdit = writable.includes(key)}
        {#if values[key] || canEdit}
          <dt>{label}</dt>
          <dd>
            {#if editing === key}
              <input class="edit" bind:value={draft} onkeydown={onKey} onblur={save} use:focus aria-label={label} spellcheck="false" />
            {:else if canEdit}
              <button class="value" class:empty={!values[key]} onclick={() => edit(key)} title="Edit {label}">
                {values[key] || PLACEHOLDER[key]}
              </button>
            {:else}
              {values[key]}
            {/if}
          </dd>
        {/if}
      {/each}
    </dl>
  {/if}
</div>

<style>
  .details {
    height: 100%;
    overflow-y: auto;
    padding: 20px 16px;
    font-size: 12px;
  }
  .head {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    text-align: center;
    margin-bottom: 18px;
  }
  .big {
    width: 96px;
    height: 96px;
    object-fit: contain;
  }
  .big.glyph {
    display: grid;
    place-items: center;
    font-size: 64px;
    color: var(--text-2);
  }
  .name {
    font-size: 15px;
    font-weight: 600;
    word-break: break-word;
    max-width: 100%;
  }
  .sub {
    color: var(--text-2);
  }
  .fields {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 8px 12px;
    margin: 0;
  }
  dt {
    color: var(--text-2);
    white-space: nowrap;
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }
  dd.wrap {
    white-space: normal;
    word-break: break-all;
  }
  .value {
    max-width: 100%;
    text-align: left;
    padding: 1px 4px;
    margin: -1px -4px;
    border-radius: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .value:hover {
    background: var(--surface-hover);
  }
  .value.empty {
    color: var(--text-3);
  }
  .edit {
    width: 100%;
    height: 24px;
    padding: 0 6px;
    margin: -3px 0;
    background: var(--input-bg);
    border: 1px solid var(--stroke);
    border-bottom: 2px solid var(--accent);
    border-radius: 4px;
    user-select: text;
  }
  @media (forced-colors: active) {
    .value:hover,
    .value:focus-visible {
      outline: 1px solid Highlight;
    }
  }
</style>
