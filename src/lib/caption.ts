import { invoke } from "@tauri-apps/api/core";

/** Keep Windows' native Snap Layout hit target over the HTML caption button. */
export function nativeCaption(node: HTMLElement, _maximized: boolean) {
  let frame = 0;
  let previous = "";
  function update() {
    frame = 0;
    const r = node.getBoundingClientRect();
    const color = (css: string, fallback: number) => {
      const parts = css.match(/[\d.]+/g)?.slice(0, 3).map(Number);
      return parts?.length === 3 ? parts[0] | (parts[1] << 8) | (parts[2] << 16) : fallback;
    };
    const root = document.querySelector(".root")!;
    const place = { x: r.x, y: r.y, width: r.width, height: r.height,
      scale: devicePixelRatio,
      background: color(getComputedStyle(root).backgroundColor, 0x202020),
      foreground: color(getComputedStyle(node).color, 0xffffff),
      visible: !document.querySelector(".menu, dialog[open]"),
    };
    const key = JSON.stringify([place, _maximized, devicePixelRatio]);
    if (previous === key) return;
    previous = key;
    invoke("caption_place", { place }).catch(() => { previous = ""; });
  }
  const schedule = () => { if (!frame) frame = requestAnimationFrame(update); };
  const size = new ResizeObserver(schedule); size.observe(node);
  const menus = new MutationObserver(schedule); menus.observe(document.body, { childList: true, subtree: true });
  const theme = new MutationObserver(schedule); theme.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  window.addEventListener("resize", schedule);
  const contrast = matchMedia("(forced-colors: active)"); contrast.addEventListener("change", schedule);
  schedule();
  return { update(maximized: boolean) { _maximized = maximized; schedule(); }, destroy() {
    size.disconnect(); menus.disconnect(); theme.disconnect(); window.removeEventListener("resize", schedule);
    contrast.removeEventListener("change", schedule); cancelAnimationFrame(frame);
  } };
}
