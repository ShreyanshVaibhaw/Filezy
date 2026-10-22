<script lang="ts">
  let { label, value, min, max, reverse = false, resize }: {
    label: string; value: number; min: number; max: number;
    reverse?: boolean; resize: (value: number) => void;
  } = $props();
  let origin: { x: number; value: number } | null = null;
  const clamp = (n: number) => Math.round(Math.min(max, Math.max(min, n)));
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).focus({ preventScroll: true });
    origin = { x: e.clientX, value };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (origin) resize(clamp(origin.value + (e.clientX - origin.x) * (reverse ? -1 : 1)));
  }
  function key(e: KeyboardEvent) {
    let next = value;
    if (e.key === "Home") next = min;
    else if (e.key === "End") next = max;
    else if (e.key === "ArrowLeft") next += reverse ? 16 : -16;
    else if (e.key === "ArrowRight") next += reverse ? -16 : 16;
    else return;
    e.preventDefault(); e.stopPropagation(); resize(clamp(next));
  }
</script>

<!-- ARIA's focusable window splitter supports pointer and keyboard resizing. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div class="splitter" role="separator" tabindex="0" aria-label={label}
  aria-orientation="vertical" aria-valuenow={value} aria-valuemin={min} aria-valuemax={max}
  onpointerdown={down} onpointermove={move} onpointerup={() => origin = null}
  onpointercancel={() => origin = null} onlostpointercapture={() => origin = null} onkeydown={key}></div>

<style>
  .splitter { width: 5px; flex-shrink: 0; cursor: col-resize; touch-action: none; z-index: 2; }
  .splitter:hover, .splitter:focus-visible { background: var(--accent); outline: none; }
</style>
