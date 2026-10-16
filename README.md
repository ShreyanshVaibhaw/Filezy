# Filezy

A fast, lightweight Windows 11 File Explorer clone built with Tauri 2, Rust, and Svelte 5.
Pixel-faithful Fluent design over direct Win32 shell integration, with none of the lag.

## Highlights

- **Fast**: opens a 100k-file folder in ~270 ms and scrolls it at 60 fps.
  Listings come straight from `FindFirstFileExW`, the UI virtualizes everything, and the whole app idles around 35 MB of RAM.
- **Real shell integration**: genuine right-click context menus (`IContextMenu`), the real Properties and Open With dialogs, `IFileOperation` for copy/move/delete (native progress and conflict dialogs, recycle bin, undo registration), `CF_HDROP` clipboard interop, and OLE drag and drop with Explorer's exact move/copy rules.
- **Everything Explorer has**: tabs (drag to reorder, per-tab history and scroll), all seven view modes with real shell thumbnails, group-by with sort-within-groups, column resize/reorder, breadcrumbs with per-segment dropdowns, Home page with Quick access and Recent files, This PC with drive capacity bars, lazy nav-pane folder tree, Quick Access pinning, marquee selection, a preview pane (text, images, audio, video and PDF, plus Windows' own previewers for Word and other files that have one), cloud folders from OneDrive and other providers with Explorer's Status column and Always keep on this device / Free up space, libraries, natural filename sorting, hidden-items and file-extension toggles, a registry-driven New menu, live folder watching, and recursive streaming search with an optional NTFS MFT index for machine-wide results.

## Installing and updating

Download the installer from [Releases](https://github.com/ShreyanshVaibhaw/Filezy/releases) and run it.

An installed copy updates itself: **Settings → About → Check for updates** downloads the next release, verifies its signature, and installs it, then offers a restart.
A copy run straight from a folder says so instead, since only an installed copy can be replaced.

## Settings

A gear at the right end of the command bar opens Settings.

- **Appearance**: theme picker with live previews, compact view, preview pane.
- **Files and folders**: hidden items, file name extensions, the default view/sort for folders you have not customized, a reset for saved folder views, clearing Recent files, "Open in Filezy" in File Explorer, reopening the last tabs, and the page new tabs open on.
- **Context menu**: an on/off switch for every entry in every right-click menu, including the entries other installed apps add through the Windows shell.
- **About**: version, updates, and a reset to stock settings.

## Opening Filezy from other programs

`filezy.exe <folder>` opens a folder, and `filezy.exe /select,<file>` opens a file's folder with the file selected, the same arguments File Explorer takes.
Each launch opens a new window, as Explorer does; the window opens where the last one closed.

**Settings → Files and folders → Open in Filezy from File Explorer** adds "Open in Filezy" to Explorer's right-click menu for folders, drives and a folder's background.
Explorer stays the default file manager.
The entry lives in your own account's registry, under `HKEY_CURRENT_USER\Software\Classes`, in the `Directory\shell\Filezy`, `Directory\Background\shell\Filezy` and `Drive\shell\Filezy` keys.
Turning the setting off deletes those three keys, and so does uninstalling Filezy; deleting them by hand removes the entry too.

## Themes

Pick from Settings → Appearance or the View menu; the choice persists.

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
npm run check:menuids        # context-menu registry drift check
npm run verify               # checks, unit tests and a frontend build
npm run build:e2e            # release exe with the end-to-end test hooks
npm run test:e2e             # drives that exe through Windows (scripts/e2e)
```

The release binary lands at `src-tauri/target/release/filezy.exe`.
The end-to-end hooks exist only in the `build:e2e` exe, so a shipped build can't be driven by them.

### Cutting a release

Updates are signed, so the build needs the private key (kept outside the repo, in `%USERPROFILE%\.tauri`):

```sh
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.tauri\filezy.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
npm run tauri build
npm run release:manifest -- "what changed"
gh release create v0.1.0 "src-tauri/target/release/bundle/nsis/Filezy_0.1.0_x64-setup.exe" latest.json
```

The app fetches `releases/latest/download/latest.json`, so each release only carries its own manifest.
Losing the private key means no installed copy can ever be updated again.

Machine-wide search uses the NTFS MFT and needs the app elevated once to index; without elevation it falls back to a recursive walk automatically.

## Author

Shreyansh Vaibhaw ([@ShreyanshVaibhaw](https://github.com/ShreyanshVaibhaw))
