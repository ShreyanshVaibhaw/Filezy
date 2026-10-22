# Filezy

A Windows file manager built with Tauri 2, Rust, and Svelte 5.
Filezy uses Windows shell operations for copying, moving, conflicts, progress dialogs and the Recycle Bin.

## Highlights

- Virtualized file lists, eight views including Content, configurable metadata columns, thumbnails, sorting and grouping.
- Tabs with history, saved sessions, reordering and transfers between windows.
- Windows context menus, Properties, Open With, clipboard interop and OLE drag and drop.
- Home, Quick access, Favorites, Recent files, drive and network browsing, Libraries and registered cloud roots.
- Text, image, audio, video and PDF previews, plus installed Windows preview handlers.
- ZIP creation and extraction, plus read and extraction support for 7z, tar and tar.gz.
- Streaming search with metadata filters, Windows Search integration and an optional NTFS MFT index.
- Resizable panes, keyboard navigation, high-contrast styles, optional single-click opening and per-folder preferences.

Devices use a separate shell adapter with browse and copy-out commands.
Device writes, rename, delete, search and previews are unavailable in that adapter.
WSL distributions appear when registered for the current user; opening one can start it.
Device, cloud-provider and shell-extension compatibility depends on Windows and the installed provider.
English is the supported UI language; messages use a translation helper and an expanded-text mode for layout testing.

## Recorded performance

The 23 October 2026 production x64 build opened 100,000 empty files in Details view in a median of 184 ms across five warm-cache runs.
Exact-name searches took a median of 167 ms across three runs.
During 120 scripted scroll steps, renderer frame intervals had a median of 12.2 ms and a 95th percentile of 20.2 ms; two exceeded 34 ms.
The viewport was 1280 by 820 CSS pixels at 125 percent scaling on Windows 11 build 26200.9711, with a Core i5-10300H, 8 logical processors and 31.8 GB RAM.
Other applications were running; the compiler had finished before measurement.
After the run, Filezy used 101.4 MiB working set; Filezy plus six WebView2 processes used 527.6 MiB, counting shared pages more than once, and 257.1 MiB private memory.
The renderer navigation-to-load interval was 368 ms; this excludes process and WebView startup and is not an end-to-end launch measurement.
Cold-cache listing and end-to-end startup remain unmeasured.
The retained ExtraCursor case in `scripts/e2e/extracursor-cases.mjs` exercises the production UI without test hooks.

On 12 October 2026, a release build opened 100,000 empty files in a median of 197 ms across five runs with a warm OS cache.
Scrolling had a median frame time of 18.3 ms and a 95th percentile of 39.4 ms.
The machine had 8 logical cores and 32 GB RAM, with other applications running.
Filezy's process used 76 MB working set; adding its six WebView2 processes gave 574 MB, including shared pages counted more than once.
These are historical measurements, not guarantees for another machine or build.
Cold-cache listing was not measured.

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

- System (follows Windows light/dark)
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
npm ci
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
The legacy E2E runner includes tests that can foreground windows or change the clipboard.
Use ExtraCursor for interactive desktop testing while someone is using the computer, with background dispatch and a disposable profile.
Do not run the legacy input tests during that session.
The Windows x64 CI job checks the frontend, Rust tests and NSIS packaging on Windows Server 2022.
Its unsigned installer is a packaging artifact, not a signed updater release.

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
The manifest tool requires one installer with the exact version and architecture in its Tauri filename and the matching `.exe.sig` file.
It checks that the package, Cargo and Tauri versions agree and verifies the Minisign signature against the app's bundled public key.
Stale, ambiguous, mismatched or modified artifacts fail before a manifest is written.
Set `FILEZY_BUNDLE_DIR` and `FILEZY_ARCH` when preparing a different output directory or architecture.
Windows x64 is the current CI target; other targets need their own build and native acceptance results.

Updater signing authenticates downloaded update bytes using the public key embedded in Filezy.
Windows Authenticode signing identifies the executable's publisher and is a separate certificate and verification step.
An updater signature does not imply Authenticode signing.
Keep release keys outside the repository and preserve the existing updater public key when building compatible updates.

Before publishing a release, check fresh installation, upgrade with settings preserved, uninstall with Explorer entries removed, portable startup, missing WebView2, cancelled or failed downloads, and invalid update signatures in a disposable Windows VM.
Record the Windows build, DPI, hardware, exact build and dataset with native acceptance and performance results.
CI packaging alone does not establish those installation or device results.

Machine-wide search uses the NTFS MFT and needs the app elevated once to index; without elevation it falls back to a recursive walk automatically.

## Author

Shreyansh Vaibhaw ([@ShreyanshVaibhaw](https://github.com/ShreyanshVaibhaw))
