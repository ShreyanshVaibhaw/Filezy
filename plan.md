# Plan: Filezy

Status (2026-07-20, later): OneDrive nav section (onedrive_dir command from %OneDrive%; rendered with treeNode so it expands - E2E showed the real Attachments/Microsoft Copilot Chat Files children); blank-context-menu "Open in Terminal" (open_terminal command: wt.exe -d, cmd /K fallback - E2E click spawned Windows Terminal); Alt+Enter opens Properties on the selection. All verified on the release exe.
Status (2026-07-20): Special-folder icons fixed: special_dirs now resolves REAL known folders via SHGetKnownFolderPath (this machine's Desktop/Documents/Pictures are OneDrive-redirected - the old USERPROFILE joins pointed at stale local folders with no custom icons and wrong content), and path_thumb forces SIIGBF_ICONONLY (icons::shell_icon_png) so folders get their special icons instead of generic thumbnails. E2E on the release exe: all 6 specials + both drives show real imgs in the nav pane and all 6 Home tiles; screenshot shows Explorer's exact Desktop/Documents/Downloads/Pictures/Music/Videos icons. Dev-with-CDP module-splitting makes this untestable in dev - verify icon work on the release build.
Status (2026-07-19, night): Real shell icons for fixed chrome: new path_thumb(path, size) command (icons::thumbnail) + store shellIcon(path, size) cache; nav pane specials/pins/drives/tree nodes (16px), Home Quick access tiles (40px), This PC drive tiles (48px) all show the genuine Windows icons (colored folders, Downloads arrow, drive icons) with glyph fallbacks while loading. Gotcha: shellIcon is called from templates, and mutating a reactive SvelteMap during render is illegal in Svelte 5 - it silently killed NavPane's render mid-component (specials/drives vanished); the fetch kick-off is deferred via queueMicrotask. Also recurring dev-only quirk: with CDP the webview sometimes loads before vite and never mounts; import('/src/main.ts') from the console mounts it (never import module URLs with alternate extensions - that forks the module graph and splits store state).
Status (2026-07-19, later still): Group headers in List view too: glist layout lays groups out left-to-right, each a horizontal section with a header band (width = its column span) and items flowing top-to-bottom in ipcG = floor((viewH - 40) / rowH) rows beneath; cellPos/visible/marquee/scrollToIndex/hStep all glist-aware; .listhdr overrides .grouphdr's left/right anchoring. E2E: headers per group, cells at per-section offsets, collapsing Yesterday removed its cell. Screenshot matches Explorer's grouped List.
Status (2026-07-19, later): Grouping now renders in icon/tile views too, not just Details. FilePane grew a pixel-segment layout (ggrid) for grouped grid views: each group is a 40px header band plus ceil(count/cols) rows of cells; cellPos() places cells group-aware; visible/entrySpan/marquee/scrollToIndex all have ggrid branches; headers reuse .grouphdr with collapse toggling. List (column flow) keeps grouped ORDER without headers (ponytail). E2E in icons_m: all 8 date buckets rendered as headers with cells beneath, collapsing Today removed its cell, scroll reached the bottom groups, virtualization culls off-screen headers/cells.
Status (2026-07-19): Explorer date buckets + auto-grouping: group_bucket for modified/created now yields Today/Yesterday/Earlier this week/Last week/Earlier this month/Last month/Earlier this year/A long time ago (civil_from_days calendar math, Monday week start); sorting by a date field auto-groups by its buckets (groupAuto flag - dropped when sorting by a non-date field, never saved as an explicit pref). E2E: 8 fixtures landed in all 8 buckets, name-sort dropped the groups.
New sort fields: Date created (Entry.created from ftCreationTime, instant) and Authors/Categories/Tags/Title via the shell property store (props.rs: SHGetPropertyStoreFromParsingName + PropVariantToStringAlloc, manual PKEY fmtids; process-lifetime cache; folders first, blanks last; PROP_SORT_MAX 20k cap errors to status; set_sort now async). E2E: created sort auto-grouped with real timestamps; author sort over the whole Downloads folder took 5.7s cold and 10ms cached, Devanagari-authored PDFs sorted correctly.
Per-folder view/sort prefs (Explorer folder view settings): localStorage folderPrefs maps lowercased path -> {sort, asc, view, group}; navigate() applies the folder's prefs or the fixed default (details/name-asc/none) BEFORE listing; sortBy/setGroup/setView (new, replaces raw app.view writes) save prefs; virtual pages excluded; 500-entry cap. E2E: rx-dates held icons_l/size-desc while other folders stayed default, and it survived reload.
Status (2026-07-13, instant context menu): Right-click menus no longer block on the shell. Native section (Open, Open with, Pin/Unpin, Copy as path, Properties + the Cut/Copy/Rename/Delete strip) renders immediately (~24ms); shell-extension entries stream in below when QueryContextMenu finishes, with duplicates of native verbs filtered out (NATIVE_VERBS + filterExtras, seq-guarded against stale responses). Same pattern in DrivesPane. Backend: STA thread coalesces queued Build requests (rapid right-clicks: only the newest builds; drained non-build reqs handled after - 4 fast clicks resolve in ~2.1s vs ~6s serial), and App.svelte fires a background shell_menu warm-up 2.5s after launch so the cold extension-DLL load (~20s worst case with NordVPN/ShareX/etc. installed) never lands on the user's first click. Warm extras cost ~1.4s - that is Windows enumerating the extension handlers themselves, not us.
Status (2026-07-13, Filezy): Renamed to Filezy (productName/title/identifier, crate -> filezy.exe, package.json, %APPDATA%\Filezy for pins) and pushed to the private GitHub repo ShreyanshVaibhaw/Filezy (single-author commit, no co-author trailers).
Theme system: themes.css defines [data-theme] variable sets - Windows 11 Dark/Light (mica, transparent --bg) plus Catppuccin Mocha/Latte, Dracula, Nord, Tokyo Night, Gruvbox Dark (opaque --bg painted on .root); System (no attribute) follows prefers-color-scheme, whose fluent.css block is guarded with :root:not([data-theme]). setTheme() sets documentElement.dataset.theme + localStorage (WebView2-persistent, no backend). Picker: View menu Theme flyout. E2E: all 9 entries listed, Mocha applied (#1e1e2e bg, #89b4fa accent), survived reload, System restored transparency; Mocha/Dracula/Nord screenshots verified.
Status (2026-07-13, final batch): View toggles: "Hidden items" (open_dir takes show_hidden and filters at list time; hidden items stay dimmed when shown; default off like Explorer - E2E: C:\ 48 -> 63 items) and "File name extensions" (displayName strips the extension in panes only; rename/preview keep full names - E2E: plan.md -> plan). Both in the View menu; session-only (ponytail: persist in settings if missed).
Middle-click a folder row opens it in a new tab (E2E: src opened in tab 2).
Drive context menu submenus now render as hover flyouts (same subwrap pattern as FilePane) - E2E: 7-Zip flyout showed its real entries. "Give access to" stays empty: the shell fills it lazily on WM_INITMENUPOPUP (known read_menu limitation).
Status (2026-07-13): Natural filename sorting: cmp_name is now a zero-allocation byte-wise natural compare - case-insensitive, digit runs compare numerically with leading-zero handling (file2 < file10, a01 < a1) - matching Explorer's StrCmpLogicalW for the common cases (ponytail: swap in StrCmpLogicalW if exact punctuation collation matters). E2E: file1/file2/File3/file10/file20/file100 in Explorer order.
Tab drag-reorder: drag a titlebar tab horizontally past a sibling's midpoint to swap positions (5px threshold keeps plain clicks as switches). E2E: two tabs swapped [1,2] -> [2,1].
Status (2026-07-12): Quick Access pinning: pins persisted to %APPDATA%\rust-explorer\pins.json (pins.rs, get_pins/set_pins); the shell context menu's own "Pin to Quick access"/"Unpin" verbs (pintohome/unpinfromhome) are routed to OUR pin store in both FilePane and DrivesPane; pinned folders render in the nav pane (right-click -> Unpin flyout) and as Home-page Quick access tiles with their parent path as subtitle. E2E: pinned src via the real shell menu verb, nav pane + Home tile appeared, unpin removed it, pins.json written.
Empty-folder state: "This folder is empty." (or "Searching…" mid-search) centered in the pane when total is 0. Light theme already existed via prefers-color-scheme.
Status (2026-07-11, late night): Per-tab scroll preservation: Tab carries scroll; newTab/switchTab save the outgoing tab's scrollTop (hooks.getScroll) and restore after re-list - restore must be deferred a tick (setTimeout 0) because the virtualizer's spacer height isn't rebuilt yet and an early scrollTop write clamps to 0. E2E: System32 scrolled to 5000, new tab to Home, switch back -> 5000.
Drive context menus on This PC tiles: shellmenu build() with dir "" treats names[0] as an absolute path and binds via SHParseDisplayName + SHBindToParent + GetUIObjectOf (child pidl points into the full pidl - free after use). DrivesPane renders the menu (submenu entries disabled - flyout later); verbs: open -> navigate, properties -> new show_properties_path command, rest through InvokeCommand. E2E: real 16-item menu (Open, Pin to Quick access, AutoPlay, Format..., WizTree/WezTerm/PowerShell third-party) and the native "One Touch (D:) Properties" dialog opened.
Status (2026-07-11, night): Sort-within-groups: set_group now takes the active sort (sortKey, asc) and orders entries inside each bucket with the same comparator as ungrouped sorting (entry_cmp, dirs first); sortBy keeps grouping and re-invokes set_group instead of clearing it (Explorer behavior; the old ponytail shortcut is gone). E2E: grouped by Type, sorted by Size both directions - groups persisted, JSON group re-ordered 54469/422/311 desc then 311/422/54469 asc.
Drop keystate semantics: drop_paths reads GetAsyncKeyState at drop time - Ctrl forces copy, Shift forces move, otherwise same-volume (drive-letter compare) moves and cross-volume copies, exactly Explorer's rules. E2E: same-volume drop moved (source gone), cross-volume drop copied (source kept). Ctrl/Shift branches untested (need physically held keys).
__e2e now also exports invoke for driving raw commands in tests.
Status (2026-07-11, evening): Home page (virtual "::Home", now the default landing page and new-tab page like Win11 Explorer): Quick access tiles for the six pinned specials (per-folder glyphs, pin icon, "Stored locally") and a Recent files list resolved from %APPDATA%\Microsoft\Windows\Recent .lnk shortcuts via IShellLink (recent.rs; GetPath without Resolve so dead links can't pop UI or hit the network; files-only, existence-checked, newest 20). Double-click opens tiles/files; empty state message when the OS has no recent items (this machine has recent-item tracking off, verified with a synthetic .lnk that resolved correctly). Virtual-path plumbing generalized: navigate/refresh/up/setFilter treat any "::" path as a no-listing page.
Status (2026-07-11, later still): Preview pane (Alt+P and View menu toggle): right-side 300px pane; text files (txt/md/json/code, first 64 KB via a read_text command) render as monospaced text, everything else gets the real shell thumbnail at 256px; name/type/size/date footer; multi-select shows the count. E2E-verified with plan.md text and a PNG thumbnail.
Breadcrumb dropdowns: every crumb now has a chevron button that lists that folder's subfolders (list_dirs) in a dropdown, click navigates; the leading chevron lists drives (This PC children). E2E-verified: src-tauri's chevron listed its five subdirs and navigated to capabilities.
Gotcha: two rust-explorer processes sharing the WebView2 user-data dir share one browser process, so a dev launch with --remote-debugging-port silently gets no CDP if a release instance is already running - kill all instances first.
Status (2026-07-11, later): File ops now go through the shell's own IFileOperation on the pumped STA thread: paste (copy and cut/move), drop, and delete all get Explorer's native progress dialog, conflict dialogs, " - Copy" rename-on-collision for same-folder paste, the recycle bin (delete-confirmation dialog follows the user's shell setting), and undo registration (FOFX_ADDUNDORECORD). The hand-rolled copy engine and the trash crate are deleted. E2E-verified: same-folder paste created "alpha - Copy.txt", cut+paste moved across folders, delete showed the native confirm dialog and landed the file in the recycle bin. Gotcha: do not treat GetAnyOperationsAborted as user-cancel - it reads true on real failures too and masks them; only swallow COPYENGINE_E_USER_CANCELLED (0x80270000).
Also added a This PC page (virtual "::ThisPC" path, no backend listing): Devices and drives tiles with capacity bars (red under 10 percent free), free/total text, double-click to open; breadcrumb shows This PC; Up from a drive root lands there (Explorer behavior); nav-pane This PC label navigates while its chevron toggles the drive tree; works with tab history both directions.
Status (2026-07-11): Added and E2E-verified: nav-pane lazy folder tree (recursive treeNode snippet, chevron-expands drives/folders via list_dirs, click navigates), marquee (rubber-band) selection in details and grid views, and a blank-area context-menu New submenu mirroring Explorer's (Folder plus the 21 registry-driven ShellNew types). Release exe rebuilt with all of the above.
Status (2026-07-06): Phases 0-7 built plus group-by, List-view column flow, column reorder, and the MFT machine-wide index (drive-root search routes to it; falls back to the walk when the volume can't be opened, i.e. non-elevated). Only the elevated MFT path is untested (no elevation available in the build environment); a whole-repo adversarial review workflow was run and its confirmed findings fixed.
Column reorder: drag a metadata header (Date/Type/Size) to live-swap positions; Name stays primary/leftmost; a reorder drag suppresses the sort click. Rows and grid-template follow the order.
New menu (2026-07-07): registry-driven like Explorer - newmenu.rs scans HKCR extension keys for ShellNew registrations (direct, default-progid, then any progid subkey) supporting all four creation methods (NullFile, Data bytes, FileName template with Templates/ShellNew search-path + env expansion, Command wizards like Shortcut). Explorer naming ("New <Type>.ext" with (n) conflicts), inline rename armed after create, real per-type icons, results cached per run. Win11 Store-Notepad's .txt entry is AppX-manifest plumbing invisible in HKCR, so a NullFile Text Document fallback is built in.
MFT: mft.rs enumerates a volume via FSCTL_ENUM_USN_DATA into an in-memory (name,parent) node map, reconstructs full paths from the parent chain, substring-scans with a non-allocating contains_ci, stats matches for size/mtime, caps at 20k results, streams via the same search-update channel and generation-cancel as the walk. Opening \\.\X: needs elevation; on failure search_start falls back to search::run.
Group-by done: Explorer's buckets for Name/Date/Type/Size (real shell type names), collapsible headers with counts, virtualized via display-row mapping so 100k-entry folders stay instant; grouping is set from the Sort menu; header-sort clears grouping (Explorer sorts within groups - upgrade if missed).
List view now flows column-major (top-to-bottom then next column) with horizontal scrolling and horizontal-arrow navigation, matching Explorer.
OLE drag-drop done: drag OUT via DoDragDrop on the pumped STA thread with a shell IDataObject (SHCreateShellItemArrayFromIDLists + BHID_DataObject) and a minimal IDropSource; drag IN via Tauri's native drag-drop events with elementFromPoint folder hit-testing and an accent drop highlight. Drops always copy (Explorer moves within a volume - add keystate handling if missed). Same-parent drops are no-ops. Drop-in logic E2E-verified; drag-out needs a human mouse test.
Phase 7 partial: search box now does recursive streaming subtree search (single-threaded walk, results append live, cancelled by navigation or a new query; search results carry their parent folder for ops/open). Machine-wide MFT/USN index NOT built - it requires elevation, which could not be tested in this environment; the walk is the fallback the blueprint specified.
Also added: resizable Date modified/Type/Size columns in details view (name column flexes).
Phase 6 done: all view modes (Details, List, Tiles, Small/Medium/Large/Extra-large icons) on the shared grid virtualizer, real shell thumbnails via IShellItemImageFactory, multiple tabs (Ctrl+T/W/Tab, middle-click close, per-tab history), View menu and status-bar toggles wired.
Known simplifications: List view flows left-to-right (Explorer flows column-major vertical); tab switch re-lists instead of caching per-tab listings (scroll resets).
Phase 5 done: CF_HDROP clipboard interop both directions with real Explorer, real shell context menus via IContextMenu on a pumped STA thread (verb strings via GetCommandString; standard verbs like copy/copyaspath/delete/rename/properties routed to native implementations because shell-built-in verbs fail without Explorer's view host; third-party verbs like Create shortcut/7-Zip invoked through InvokeCommand and verified), real Properties dialog via ShellExecuteEx SEE_MASK_INVOKEIDLIST, Open With dialog via openas verb.
Done: scaffold, mica window with custom titlebar/tab, FindFirstFileExW listing streamed through a get_range virtual window, virtualized details view (100k files in 273ms, select-all instant), breadcrumbs with overflow collapse, back/forward/up history, nav pane (Home, pinned specials, This PC drives), async icon cache + real shell icons, notify-based live watcher, in-folder instant search filter, copy/cut/paste with " - Copy" conflict naming, recycle-bin delete, inline rename, new folder, custom context menus, sort menu, status bar, full keyboard shortcuts.
Not yet: tabs (multi), other views (list/tiles/icons), shell IContextMenu verbs, OLE drag-drop, CF_HDROP clipboard interop with real Explorer, machine-wide MFT search, group-by, column resize.

Build order chosen so every phase ends with a runnable app that is already useful.
Performance work is front-loaded because the virtualized list and fast listing are the foundation everything else sits on.

## Phase 0 - Scaffold (half a day)

- `npm create tauri-app@latest` with Svelte + TypeScript.
- Turn off window decorations, enable mica backdrop, set min window size.
- CI-free, repo-local: `cargo check` and `npm run check` must pass, nothing else.

Done when: empty mica window opens in under a second.

## Phase 1 - Fast listing + virtualized details view (the core)

- Rust: `list_dir` command using `FindFirstFileExW` with `FIND_FIRST_EX_LARGE_FETCH`.
- Stream results: first 200 entries returned synchronously, remainder chunked over an event channel.
- Svelte: `VirtualList` component, fixed row height, overscan of 20, renders only visible rows.
- Details view with Name, Date modified, Type, Size columns and click-to-sort (sort in Rust).
- Placeholder icons by extension.

Done when: a folder with 100k files opens with no perceptible delay and scrolls at 60fps.
This is the benchmark gate for the whole project: if this phase is not instant, stop and fix it before adding anything.

## Phase 2 - Navigation

- Breadcrumb address bar with per-segment dropdowns and click-to-edit path mode.
- Back/forward/up history stack.
- Left nav pane: drives with capacity bars, This PC tree (lazy-expanded), Quick Access pinning (stored in one JSON settings file).
- Keyboard: Enter, Backspace, Alt+Up, Alt+Left/Right, Ctrl+L.

Done when: you can get anywhere on the machine without touching real Explorer.

## Phase 3 - Real icons and live updates

- Background icon resolver thread, extension-keyed LRU cache, per-path overrides for exe/lnk/custom folders.
- `ReadDirectoryChangesW` watcher per open folder, debounced row patches.

Done when: icons fill in within ~100ms of paint and an external file create/delete appears without refreshing.

## Phase 4 - File operations

- Selection model: click, ctrl, shift, marquee, select-all.
- Cut/copy/paste via `CF_HDROP` clipboard interop.
- Copy/move/delete/rename through `IFileOperation` (gives conflict dialogs, recycle bin, and undo for free).
- Inline rename (F2), new folder (Ctrl+Shift+N), delete and shift+delete.
- Progress events wired to a Fluent progress card.

Done when: every operation round-trips correctly against real Explorer (copy here, paste there, undo works).

## Phase 5 - Shell integration

- Right-click context menu via `IContextMenu`, rendered in Windows 11 compact style.
- Properties via `SHObjectProperties`, Open With via the shell verb.
- OLE drag and drop, both directions, including drops from real Explorer.

Done when: context menu shows the same third-party entries as real Explorer and drag-drop works both ways.

## Phase 6 - Views and polish

- List, Tiles, and icon-grid views (small through extra large) sharing the virtualization core.
- Group-by headers, column resize and reorder.
- Command bar with full Explorer button set, three-dot overflow.
- Status bar with counts and selection size.
- Tabs (Ctrl+T), matching Explorer tab visuals.

Done when: side-by-side screenshots with real Explorer are hard to tell apart.

## Phase 7 - Instant search

- MFT reader building the all-volumes filename index (elevated mode).
- USN journal tailing to keep it current.
- Non-elevated fallback: recursive walk of the current folder only.
- Search box UI, results replacing the file pane, highlight matching substring.

Done when: machine-wide name search returns in under 50ms on a warm index.

## Phase 8 - Hardening

- Long-path (`\\?\`) support everywhere, junction/symlink display, UNC paths.
- Per-monitor DPI, light/dark theme following the OS.
- Memory audit against the budget in blueprint.md.
- Manual test pass: the checklist is every row of the "UI surface to clone" section in blueprint.md.

## Explicitly out of scope (v1)

- Gallery view, OneDrive status columns, Network browsing beyond typed UNC paths, file preview pane, tags/ratings columns.
- Add any of these only after v1 ships and only if actually missed.

## Order of risk

The two things most likely to sink the project are shell context menus (COM fiddliness) and MFT reading (needs elevation, undocumented-ish structures).
Both are isolated in their own modules with graceful fallbacks (basic menu, directory-walk search), so a failure there degrades the app instead of blocking it.
