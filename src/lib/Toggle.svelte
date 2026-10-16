<script lang="ts">
  // Win11 toggle switch. Keyboard-operable because it is a real button; `label`
  // names it for screen readers, since the visible text sits elsewhere in the row.
  let {
    checked = false,
    label,
    onchange,
  }: { checked?: boolean; label: string; onchange: (v: boolean) => void } = $props();
</script>

<button
  class="toggle"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  onclick={() => onchange(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    width: 40px;
    height: 20px;
    border-radius: 10px;
    border: 1px solid var(--text-3);
    background: transparent;
    padding: 0;
    flex-shrink: 0;
    position: relative;
    cursor: default;
  }
  .toggle:hover {
    background: var(--hover);
  }
  .toggle.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .knob {
    position: absolute;
    top: 50%;
    left: 4px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--text-2);
    transform: translateY(-50%);
    transition: left 120ms ease, width 120ms ease;
  }
  .toggle.on .knob {
    left: 22px;
    background: var(--menu-bg);
  }
  .toggle:active .knob {
    width: 16px;
  }
  .toggle.on:active .knob {
    left: 18px;
  }
</style>
