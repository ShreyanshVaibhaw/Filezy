<!-- Extract all, as Explorer asks it: where the files go (a new folder named
     after the archive, beside it, unless changed), and whether to show them. -->
<script lang="ts">
  import { msg } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { browseDest, extractDlg, runExtract } from "./archive.svelte";
  import Modal from "./Modal.svelte";

  let input: HTMLInputElement | undefined = $state();
  // Whether the destination is there already: it decides what the hint says.
  let exists = $state(false);
  $effect(() => {
    const dest = extractDlg.dest;
    if (!extractDlg.open || !dest.trim()) return;
    const timer = setTimeout(async () => {
      const st = await invoke<{ exists: boolean }[]>("path_stamps", { paths: [dest] }).catch(() => []);
      if (dest === extractDlg.dest) exists = !!st[0]?.exists;
    }, 120);
    return () => clearTimeout(timer);
  });
  const invalid = $derived(!extractDlg.dest.trim() ? "Choose a folder to extract the files to." : /[*?"<>|]/.test(extractDlg.dest) ? 'A folder path can\'t contain any of these characters: * ? " < > |' : "");

  $effect(() => {
    if (!extractDlg.open) return;
    queueMicrotask(() => {
      input?.focus();
      input?.select();
    });
  });

  function close() {
    extractDlg.open = false;
  }

  function go() {
    if (!invalid) runExtract();
  }
</script>

{#if extractDlg.open}
  <Modal
    width={560}
    class="extractdlg"
    labelledby="ex-title"
    onclose={close}
    onkeydown={(ev) => {
      if (ev.key === "Enter" && ev.target === input) {
        ev.preventDefault();
        go();
      }
    }}
  >
    <h2 id="ex-title">{extractDlg.title}</h2>
    <label class="field" for="ex-dest">{msg("Files will be extracted to this folder:")}</label>
    <div class="row">
      <input id="ex-dest" bind:this={input} bind:value={extractDlg.dest} spellcheck="false" aria-describedby="ex-hint" />
      <button class="btn" onclick={browseDest}>{msg("Browse...")}</button>
    </div>
    <p id="ex-hint" class="hint" class:err={!!invalid}>
      {invalid || (exists ? "This folder exists. Windows asks before replacing anything already in it." : "This folder will be created.")}
    </p>
    <label class="check">
      <input type="checkbox" bind:checked={extractDlg.show} />
      <span>{msg("Show extracted files when complete")}</span>
    </label>
    <div class="btns">
      <button class="btn primary" disabled={!!invalid} onclick={go}>{msg("Extract")}</button>
      <button class="btn" onclick={close}>{msg("Cancel")}</button>
    </div>
  </Modal>
{/if}

<style>
  h2 {
    margin-bottom: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .field {
    display: block;
    font-size: 13px;
    color: var(--text-2);
    margin-bottom: 6px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row input {
    flex: 1;
    min-width: 0;
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
    margin: 8px 0 12px;
  }
  .hint.err {
    color: var(--danger);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    width: fit-content;
  }
  .check input {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
  }
</style>
