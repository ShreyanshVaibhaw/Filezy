// Keyboard for a dropdown or context menu (`use:menuKeys={close}` on a .menu):
// Up/Down/Home/End move between items, Right or Enter on an item with a
// submenu opens it, Left closes a submenu, Escape closes the menu, a letter
// jumps to the next item starting with it, and focus goes back to where it was.
//
// Submenus open on hover (their .subwrap listens for mouseenter), so the
// keyboard opens them by sending that same event.

export function menuKeys(node: HTMLElement, close: () => void) {
  const returnTo = document.activeElement as HTMLElement | null;
  const isSub = node.classList.contains("submenu");
  node.setAttribute("role", "menu");

  /** This menu's own enabled buttons, not those of a nested submenu. */
  const items = () =>
    [...node.querySelectorAll<HTMLButtonElement>("button")].filter(
      (b) => !b.disabled && b.closest(".menu") === node,
    );

  for (const b of items()) {
    if (!b.getAttribute("role")) b.setAttribute("role", "menuitem");
    if (b.closest(".subwrap") && b.querySelector(".subchev")) b.setAttribute("aria-haspopup", "menu");
  }
  // Mouse-opened menus focus too, so the keyboard can take over at any point.
  queueMicrotask(() => items()[0]?.focus({ preventScroll: true }));

  function openSub(b: HTMLButtonElement): boolean {
    const wrap = b.closest(".subwrap");
    if (!wrap || !b.querySelector(".subchev")) return false;
    wrap.dispatchEvent(new MouseEvent("mouseenter"));
    return true;
  }

  function onKey(e: KeyboardEvent) {
    const list = items();
    const cur = list.indexOf(document.activeElement as HTMLButtonElement);
    const go = (i: number) => list[(i + list.length) % list.length]?.focus();
    switch (e.key) {
      case "ArrowDown":
        go(cur + 1);
        break;
      case "ArrowUp":
        go(cur < 0 ? -1 : cur - 1);
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(-1);
        break;
      case "ArrowRight":
        if (cur < 0 || !openSub(list[cur])) return;
        break;
      case "Enter":
      case " ":
        if (cur >= 0 && openSub(list[cur])) break;
        return; // the focused button activates itself
      case "ArrowLeft":
        if (!isSub) return;
        close();
        break;
      case "Escape":
        close();
        break;
      case "Tab":
        go(cur + (e.shiftKey ? -1 : 1));
        break;
      default: {
        if (e.key.length !== 1 || e.ctrlKey || e.altKey) return;
        const k = e.key.toLowerCase();
        const n = list.length;
        for (let s = 1; s <= n; s++) {
          const b = list[(cur + s) % n];
          if (b.textContent?.trim().toLowerCase().startsWith(k)) {
            b.focus();
            break;
          }
        }
      }
    }
    e.preventDefault();
    e.stopPropagation();
  }

  node.addEventListener("keydown", onKey);
  return {
    destroy() {
      node.removeEventListener("keydown", onKey);
      // Give focus back unless the user already moved it somewhere else.
      const a = document.activeElement;
      if (!a || a === document.body || node.contains(a)) returnTo?.focus?.({ preventScroll: true });
    },
  };
}

/// Keep a menu inside the window while its entries stream in: move it up or
/// left when it would run past an edge, and let it scroll as a whole when it is
/// taller than the window. `use:fitMenu={key}`: a new key (a new right-click
/// reusing the element) places it again.
export function fitMenu(node: HTMLElement, _key?: unknown) {
  const fit = () => {
    node.style.maxHeight = `${innerHeight - 8}px`;
    node.style.overflowY = "auto";
    const r = node.getBoundingClientRect();
    if (r.bottom > innerHeight - 4) node.style.top = `${Math.max(4, innerHeight - 4 - r.height)}px`;
    if (r.right > innerWidth - 4) node.style.left = `${Math.max(4, innerWidth - 4 - r.width)}px`;
  };
  const ro = new ResizeObserver(fit);
  ro.observe(node);
  fit();
  return {
    update() {
      queueMicrotask(fit);
    },
    destroy() {
      ro.disconnect();
    },
  };
}
