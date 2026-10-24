<script lang="ts">
  import { msg, msgCount } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { batch, commitBatch } from "./ops.svelte";
  import { baseName, joinPath, parentPath } from "./paths";
  import { ruleNames, type RenameMode, type RenameRule } from "./renamerules";
  import Modal from "./Modal.svelte";

  const MODES: [RenameMode, string][] = [
    ["number", "Numbered"],
    ["replace", "Find and replace"],
    ["template", "Template"],
  ];
  let rule = $state<RenameRule>({ mode: "number", base: "", find: "", replace: "", regex: false, matchCase: false, template: "{name} {n:2}", start: 1 });
  let input: HTMLInputElement | undefined = $state();
  // Modified times for {date}, read when the dialog opens.
  let modified = $state<number[]>([]);
  // New names that already belong to an item outside the batch.
  let taken = $state(new Set<string>());

  $effect(() => {
    if (!batch.open) return;
    // Start from the first item's name without its extension, selected.
    const first = batch.items[0];
    const n = baseName(first.path);
    const dot = first.is_dir ? -1 : n.lastIndexOf(".");
    rule.base = dot > 0 ? n.slice(0, dot) : n;
    rule.mode = "number";
    modified = [];
    invoke<{ modified: number }[]>("path_stamps", { paths: batch.items.map((it) => it.path) })
      .then((st) => (modified = st.map((s) => s?.modified ?? 0)))
      .catch(() => {});
    queueMicrotask(() => {
      input?.focus();
      input?.select();
    });
  });

  const result = $derived(ruleNames(rule, batch.items.map((it, i) => ({ ...it, modified: modified[i] }))));
  const names = $derived(result.names);
  const targets = $derived(batch.items.map((it, i) => joinPath(parentPath(it.path) ?? "", names[i])));
  const invalid = $derived(result.error);
  const clashes = $derived(targets.filter((t) => taken.has(t.toLowerCase())).length);
  const changed = $derived(batch.items.filter((it, i) => baseName(it.path) !== names[i]).length);

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

  function setMode(m: RenameMode) {
    rule.mode = m;
    queueMicrotask(() => input?.focus());
  }

  function go() {
    if (!invalid && changed) commitBatch(names);
  }


  const HINTS: Record<RenameMode, string> = {
    number: "Each item gets this name and a number, in list order, and keeps its own extension.",
    replace: "Replaces text in each name, leaving the extension alone. With regular expressions, $1 inserts a group.",
    template: "{name} the old name, {n} a number ({n:3} as 001), {date} the date modified, {parent} the folder. The extension is kept.",
  };
</script>

{#if batch.open}
  <Modal
    width={600}
    labelledby="br-title"
    onclose={() => (batch.open = false)}
    onkeydown={(ev) => {
      if (ev.key === "Enter" && ev.target instanceof HTMLInputElement && ev.target.type === "text") {
        ev.preventDefault();
        go();
      }
    }}
  >
    <h2 id="br-title">{msg("Rename {n} items", { n: batch.items.length.toLocaleString() })}</h2>
    <div class="modes" role="radiogroup" aria-label={msg("Rename by")}>
      {#each MODES as [m, label] (m)}
        <button class="mode" role="radio" aria-checked={rule.mode === m} class:on={rule.mode === m} onclick={() => setMode(m)}>{msg(label)}</button>
      {/each}
    </div>
    {#if rule.mode === "number"}
      <label class="field">
        <span>{msg("New name")}</span>
        <input type="text" bind:this={input} bind:value={rule.base} spellcheck="false" aria-describedby="br-hint" />
      </label>
    {:else if rule.mode === "replace"}
      <div class="pair">
        <label class="field">
          <span>{msg("Find")}</span>
          <input type="text" bind:this={input} bind:value={rule.find} spellcheck="false" aria-describedby="br-hint" />
        </label>
        <label class="field">
          <span>{msg("Replace with")}</span>
          <input type="text" bind:value={rule.replace} spellcheck="false" />
        </label>
      </div>
      <div class="opts">
        <label><input type="checkbox" bind:checked={rule.matchCase} /> {msg("Match case")}</label>
        <label><input type="checkbox" bind:checked={rule.regex} /> {msg("Regular expression")}</label>
      </div>
    {:else}
      <div class="pair">
        <label class="field">
          <span>{msg("Template")}</span>
          <input type="text" bind:this={input} bind:value={rule.template} spellcheck="false" aria-describedby="br-hint" />
        </label>
        <label class="field num">
          <span>{msg("Start at")}</span>
          <input type="number" bind:value={rule.start} min="0" />
        </label>
      </div>
    {/if}
    <p id="br-hint" class="hint" class:err={!!invalid}>{invalid || msg(HINTS[rule.mode])}</p>
    <div class="preview" role="list" aria-label={msg("Preview")}>
      {#each batch.items as it, i (it.path)}
        {@const bad = taken.has(targets[i].toLowerCase())}
        <div class="prow" class:bad role="listitem">
          <span class="old" title={baseName(it.path)}>{baseName(it.path)}</span>
          <span class="glyph arrow" aria-hidden="true">&#xE72A;</span>
          <span class="new" title={names[i]}>{names[i]}</span>
          {#if bad}<span class="why">{msg("name taken")}</span>{/if}
        </div>
      {/each}
    </div>
    {#if clashes}
      <p class="hint err">
        {msgCount(clashes, "{n} new name is already used in that folder; that item keeps its name.", "{n} new names are already used in that folder; those items keep their names.")}
      </p>
    {/if}
    <div class="btns">
      <span class="count">{msg("{changed} of {n} names change", { changed: changed.toLocaleString(), n: batch.items.length.toLocaleString() })}</span>
      <button class="btn primary" disabled={!!invalid || !changed} onclick={go}>{msg("Rename")}</button>
      <button class="btn" onclick={() => (batch.open = false)}>{msg("Cancel")}</button>
    </div>
  </Modal>
{/if}

<style>
  .modes {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
    padding: 3px;
    border: 1px solid var(--stroke);
    border-radius: 6px;
    align-self: flex-start;
  }
  .mode {
    height: 26px;
    padding: 0 12px;
    border-radius: 4px;
    font-size: 13px;
    color: var(--text-2);
  }
  .mode:hover:not(.on) {
    background: var(--hover);
  }
  .mode.on {
    background: var(--sel);
    color: var(--text);
  }
  .pair {
    display: flex;
    gap: 12px;
  }
  .pair .field {
    flex: 1;
    min-width: 0;
  }
  .pair .field.num {
    flex: 0 0 96px;
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
  .opts {
    display: flex;
    gap: 20px;
    margin-top: 10px;
    font-size: 13px;
    color: var(--text-2);
  }
  .opts label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .opts input {
    accent-color: var(--accent);
  }
  .hint {
    font-size: 12px;
    color: var(--text-3);
    margin: 8px 0 10px;
  }
  .hint.err {
    color: var(--danger);
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
    color: var(--danger);
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
    color: var(--danger);
  }
  .count {
    margin-right: auto;
    font-size: 12px;
    color: var(--text-3);
  }
</style>
