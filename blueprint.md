# Blueprint: Filezy

A pixel-faithful clone of Windows 11 File Explorer that is faster, lag-free, and uses a fraction of the memory.

## Tech stack

| Layer | Choice | Why |
|---|---|---|
| App shell | Tauri 2 | Rust backend, tiny binary, uses the WebView2 runtime already shipped with Windows 11. |
| Backend | Rust + `windows-rs` | Direct Win32 calls, no abstraction tax. |
| Frontend | Svelte 5 + TypeScript | Compiles away, no virtual DOM, smallest fast option. |
| Styling | Hand-written CSS (Fluent tokens) | Pixel-matching Explorer is a CSS job. No component library. |
| State | Svelte runes + one Rust-side store | No Redux-style layer. |
| IPC | Tauri commands + events | Built in. No extra protocol. |

Skipped: React (heavier, slower), any UI kit (none matches Explorer exactly), Electron (disqualified by memory), custom native GUI (months of widget painting for zero user-visible gain).

## Performance architecture

These five decisions are what make it "ultra fast".
Everything else is decoration.

### 1. Directory listing via Win32, not std

Use `FindFirstFileExW` with `FIND_FIRST_EX_LARGE_FETCH` and `FindExInfoBasic`.
This skips short-name resolution and batches kernel transitions.
It is several times faster than `std::fs::read_dir` on large directories.
One call returns name, size, timestamps, and attributes together, so there are zero per-file stat calls.

### 2. Virtualized rendering, always

The frontend renders only the visible rows plus a small overscan buffer.
A folder with 500,000 files paints as fast as a folder with 50.
The backend sends the first visible page immediately, then streams the rest in chunks over a Tauri event channel.
Sorting and filtering happen in Rust, and the frontend only ever holds the slice it displays.

### 3. Icons are async, cached, and never block

The list renders on first paint with per-extension placeholder icons.
A Rust background thread resolves real icons via `SHGetFileInfoW` / `IShellItemImageFactory` and pushes them up as they arrive.
Icons are cached by extension (one lookup covers every `.txt` on the machine) with a per-path override cache for `.exe`, `.lnk`, and folders with custom icons.
The cache is an in-memory LRU persisted to disk as PNG bytes keyed by extension and size.

### 4. Live updates via ReadDirectoryChangesW

No polling, no manual refresh.
Each open folder holds one watcher handle.
Change events are debounced (~50ms) in Rust and sent to the frontend as row-level patches, not full reloads.

### 5. Instant search via the NTFS MFT and USN journal

Name search does not walk directories.
On first run, read the Master File Table directly to build an in-memory index of every filename on each NTFS volume (this is the "Everything" technique and takes seconds for millions of files).
Keep the index current by tailing the USN change journal.
Search becomes a substring scan over an in-memory array: milliseconds, machine-wide.
MFT access requires the app to run elevated or the index falls back to a plain directory walk for non-admin sessions.

## Process model

One Rust process, one WebView2 window.
File operations (copy, move, delete) run on a Rust thread pool via `IFileOperation` so the Windows progress, conflict, and undo semantics come for free and the UI thread never blocks.
Long copies report progress over an event channel to a custom Fluent progress UI.

## UI surface to clone

Target: Windows 11 Explorer (23H2 layout), matched view by view.

- Title bar: custom (Tauri decorations off), mica backdrop, tabs like Explorer tabs, snap-layout support via `WM_NCHITTEST` passthrough.
- Command bar: New, Cut, Copy, Paste, Rename, Share, Delete, Sort, View, three-dot overflow.
- Address bar: breadcrumb segments with per-segment dropdowns, click-to-edit path mode, history back/forward/up.
- Search box: right of address bar, results replace the file pane.
- Left nav pane: Home, Gallery, pinned Quick Access, This PC tree, Network, drives with capacity bars.
- File pane views: Details, List, Tiles, and icon sizes small through extra large.
- Details view: sortable columns, column resize and reorder, group-by headers.
- Selection: click, ctrl, shift, marquee drag-select, select-all.
- Context menu: Windows 11 compact style with the real shell verbs (Open with, Send to, shell extension entries) via `IContextMenu`.
- Status bar: item count, selection count and size, view toggle buttons.
- Dialogs: rename inline, delete confirm, copy conflict dialog (handled by `IFileOperation`), properties via `SHObjectProperties` (real shell dialog, zero custom UI).
- Drag and drop: OLE drag-drop both directions (`IDropTarget`, `IDropSource`) so it interoperates with real Explorer and other apps.
- Keyboard: full Explorer parity (F2, F5, Ctrl+L, Alt+Up, Alt+Left/Right, Ctrl+Shift+N, Delete, Shift+Delete).
- Clipboard: `CF_HDROP` interop so copy/paste works against real Explorer.

Deliberately delegated to the shell instead of rebuilt: properties dialog, copy conflict dialog, Open With picker, and shell context-menu verbs.
Rebuilding those adds months and guarantees subtle incompatibility.

## Memory budget

| Component | Target |
|---|---|
| Rust process idle | under 30 MB |
| WebView2 (attributed, shared runtime) | 40-70 MB |
| MFT index per 1M files | ~60 MB |
| Icon cache cap | 20 MB LRU |

The frontend never materializes more than ~200 row objects regardless of folder size.

## Crate list

`tauri`, `windows` (Win32 + Shell APIs), `parking_lot`, `serde`, `rayon` (sort/filter of big listings), `lru`.
Nothing else until something measurably demands it.

## Project layout

```
filezy/
  src-tauri/
    src/
      main.rs           app setup, window config
      fs/list.rs        FindFirstFileExW listing + streaming
      fs/watch.rs       ReadDirectoryChangesW watchers
      fs/ops.rs         IFileOperation wrappers
      fs/icons.rs       icon resolution + LRU cache
      search/mft.rs     MFT reader + USN tail
      shell/menu.rs     IContextMenu integration
      shell/dragdrop.rs OLE drag-drop
  src/                  Svelte frontend
    lib/VirtualList.svelte
    lib/components/     command bar, nav pane, address bar, status bar
    lib/views/          details, list, tiles, icons
    styles/fluent.css   design tokens matched to WinUI
```
