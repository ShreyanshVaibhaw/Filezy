# Filezy

A fast, lightweight Windows 11 File Explorer clone built with Tauri 2, Rust, and Svelte 5.
Pixel-faithful Fluent design over direct Win32 shell integration, with none of the lag.

## Highlights

- **Fast**: opens a 100k-file folder in ~270 ms and scrolls it at 60 fps.
  Listings come straight from `FindFirstFileExW`, the UI virtualizes everything, and the whole app idles around 35 MB of RAM.
- **Real shell integration**: genuine right-click context menus (`IContextMenu`), the real Properties and Open With dialogs, `IFileOperation` for copy/move/delete (native progress and conflict dialogs, recycle bin, undo registration), `CF_HDROP` clipboard interop, and OLE drag and drop with Explorer's exact move/copy rules.
- **Everything Explorer has**: tabs (drag to reorder, per-tab history and scroll), all seven view modes with real shell thumbnails, group-by with sort-within-groups, column resize/reorder, breadcrumbs with per-segment dropdowns, Home page with Quick access and Recent files, This PC with drive capacity bars, lazy nav-pane folder tree, Quick Access pinning, marquee selection, preview pane, natural filename sorting, hidden-items and file-extension toggles, a registry-driven New menu, live folder watching, and recursive streaming search with an optional NTFS MFT index for machine-wide results.

## Themes

Pick from the View menu; the choice persists.

- System (follows Windows light/dark, mica backdrop)
- Windows 11 Dark / Windows 11 Light
- Catppuccin Mocha / Catppuccin Latte
- Dracula
- Nord
- Tokyo Night
- Gruvbox Dark

## Stack

| Layer | Choice |
|---|---|
| Shell | Tauri 2 (WebView2, mica) |
| Backend | Rust + `windows-rs`, direct Win32 calls |
| Frontend | Svelte 5 (runes) + TypeScript + Vite |
| Styling | Hand-written Fluent CSS, Segoe Fluent Icons |

## Building

```sh
npm install
npm run tauri dev            # development
npm run tauri build          # release + NSIS installer
npm run tauri build -- --no-bundle   # release exe only
```

The release binary lands at `src-tauri/target/release/filezy.exe`.

Machine-wide search uses the NTFS MFT and needs the app elevated once to index; without elevation it falls back to a recursive walk automatically.

## Author

Shreyansh Vaibhaw ([@ShreyanshVaibhaw](https://github.com/ShreyanshVaibhaw))
