# Plan 2: Settings

Status (2026-08-02): all seven phases done, plus in-app updates (Phase 8, added after the fact).
Phase 8: `tauri-plugin-updater` + `tauri-plugin-process`, signed with a minisign key kept in `%USERPROFILE%\.tauri` (never in the repo; losing it means no installed copy can be updated again).
The endpoint is `releases/latest/download/latest.json`, so each release carries only its own manifest, built by `npm run release:manifest`.
Settings > About gained a Check for updates row (idle / checking / found / downloading with a progress bar / ready / error) and an `is_installed` command gates it: a copy run from a folder is told it cannot update itself, since the NSIS bundle drops an `uninstall.exe` beside an installed copy and only an installed copy can be replaced.
Constraint to resolve: the GitHub repo is private, so release assets need authentication and the updater cannot fetch them. Making the repo public (or hosting `latest.json` and the installer somewhere public) is what turns this on; no code change is needed either way.
Phase 7: About card with version and a two-click "Reset all settings"; virtual pages get their own tab icons (house / PC / gear, and the bin's real shell icon) instead of the folder icon every tab used to show; Escape leaves the Settings page; cross-window settings sync through the `storage` event.
Cross-window sync verified for real with two processes: window 1 picked Catppuccin Mocha and window 2's captured pixels went from #272727 to #181825 on their own. Note that a second window started by Ctrl+N inherits the CDP port argument and cannot claim it, so window 2 is only observable by screenshot, not over CDP.
Phase 6: shell extension entries. A master switch that also skips the `shell_menu` call and the startup warm-up, plus a learned list of every extension entry seen so far (normalized label, since `ShellItem.id` is rebuilt per right-click), each with its own switch, and "Forget this list". E2E on this machine: 20 entries learned from one right-click, WinRAR switched off vanished from the real menu and stayed gone across a reload, master switch off left only the four native entries with no loading row.
Phase 5: 22 native entries across five menus (file item, blank area, drive tile, nav pin, tab) each switchable from a Context menu tab, backed by one registry in `src/lib/menuitems.ts`; separators collapse in CSS rather than per-site conditions, a menu with nothing left does not open, and `npm run check:menuids` fails if a menu id and the registry ever drift.

Phases 1-4:
Phase 4: Files and folders tab (Display / Default folder view / Startup) with default view+sort+order for uncustomized folders (`DEFAULT_PREFS` became `defaultPrefs()` reading settings), a two-click "Clear" for saved folder views, and a start page (Home / This PC / typed folder) used by the app and every new tab; `navigate()` now returns false on failure so a missing start folder falls back to Home instead of an error page.
Phase 3: theme grid of 9 cards, each carrying its own `data-theme` so the preview is painted by that theme's variables with no duplicated colors; accent ring plus tick on the active card, and the View menu flyout stays in sync both ways.
Fixed along the way (pre-existing): the mica backdrop behind the title bar and nav pane is painted by Windows, so a light palette on a dark-mode OS left the chrome unreadable - `applyTheme()` now also calls the window's `setTheme` (light/dark read from the theme's own `color-scheme`, null for System), which needed `core:window:allow-set-theme` in capabilities. Measured on the Windows 11 Light theme: nav label contrast range went from 6 (invisible) to 217.
Phase 2: `::Settings` virtual page (`navigate()` generalized to `startsWith("::")`), gear button at the right end of the command bar plus a "..." menu entry, `SettingsPane` with a category rail, and a `Toggle` switch component.
E2E suites live in the job scratch dir and are hermetic (each resets localStorage and reloads first): phase1 20 checks, phase2 16, phase3 11, phase4 10, all green on the dev build.

Phase 1 detail:
`src/lib/settings.svelte.ts` owns the persisted prefs (theme, compact, showHidden, showExt, previewPane) in one localStorage `settings` blob merged over `DEFAULTS`, migrating and deleting the old `theme` and `compact` keys on first run.
The five fields left `app`; `store.svelte.ts` imports settings one-way and keeps `toggleHidden` (which still refreshes).
E2E on the dev build over CDP, 20 checks green: migration carried theme and compact across and removed the old keys, compact drove details rows 28px to 40px, Hidden items took C:\ from 52 to 67 entries, extensions stripped App.svelte to App and back, and every toggle plus the theme survived a reload (they did not before this phase).
Drive-by fix in the same pass: `FilePane` `colDrag` was a plain `let` but is read in the template, so the `.coldragging` highlight never appeared while dragging a column header; it is `$state` now (E2E: no class under the 5px threshold, class past it, columns reordered to type/modified/size), which also cleared the only Svelte compiler warning in the build.
E2E gotcha for later phases: the dev webview often beats vite to the dev URL and sits on about:blank with no `__e2e`, so a CDP driver must `Page.navigate` to `http://localhost:1420/` before doing anything.

Adds a Settings surface to Filezy: one place to pick a theme, control the options that are currently session-only or menu-buried, and enable or disable every entry in the right-click menus so only the ones the user wants remain.

Written to be executed phase by phase by separate agents.
Each phase ends with a runnable app and a concrete "Done when" gate.
Read the Decisions and Traps sections before starting any phase: they exist so that seven agents produce one coherent feature instead of seven shapes of the same idea.

## Decisions

These are settled.
Do not re-litigate them mid-phase.

**Settings is a virtual page, not a modal dialog.**
`::Settings` joins `::Home`, `::ThisPC`, and `::RecycleBin`.
The app already routes on `app.path` in `App.svelte:117-136`, already has history, tabs, and breadcrumbs for virtual pages, and already skips listing for any `::` path.
A modal would need its own focus trap, its own escape handling, and its own stacking against the context menus.
Windows 11 Settings is itself a page with a category rail, so a page is also the closer visual match.

**One localStorage key, `settings`, holding one JSON object.**
Theme, compact, and folderPrefs already live in localStorage, and the theme is applied synchronously at module load (`store.svelte.ts:282`), which is why there is no theme flash at startup.
A `%APPDATA%\Filezy\settings.json` backend file (the `pins.rs` pattern) would be async and would reintroduce that flash for zero user-visible gain.
Ceiling of this choice: settings live in the WebView2 profile, so they do not roam and a profile wipe resets them.
Upgrade path if that ever matters: mirror the same JSON through a Rust command, keeping localStorage as the synchronous read cache.

**Defaults are implicit; only exceptions are stored.**
The stored object is merged over a `DEFAULTS` constant, and disabled menu items are stored as a list of hidden ids rather than a full map of booleans.
A new menu item added in a later release therefore ships enabled, and old settings files stay forward compatible.

**Native menu items are gated by stable string ids from one registry.**
`src/lib/menuitems.ts` is the single source of truth for the id, label, and glyph of every native context-menu entry.
The settings page renders its checkboxes from that registry; the menus themselves keep their inline click handlers and only ask `menuOn("file.copypath")`.
Rendering the menus from the registry as well would mean pushing every bespoke handler and every `{#if}` condition into data, which is a much larger diff for no user-visible difference.
A drift check (Phase 5) enforces that the two sides agree.

**Shell extension entries are remembered by normalized text, never by id.**
`ShellItem.id` is an index into the menu that the Rust side built for that one right-click, so it changes between builds and between selections.
The only stable handle for "the 7-Zip entry" is its label.
Normalization: strip `&` accelerators, strip a trailing ellipsis, trim.

**Target shape of the settings object.**
Phase 1 ships the first block, later phases append their own; absent keys always fall back to the default.

```ts
const DEFAULTS = {
  // Phase 1
  theme: "system",
  compact: false,
  showHidden: false,
  showExt: true,
  previewPane: false,
  // Phase 4
  startPage: "::Home",
  defaultView: "details",
  defaultSort: "name",
  defaultAsc: true,
  // Phase 5
  hiddenMenu: [] as string[],
  // Phase 6
  shellExtras: true,
  hiddenExtras: [] as string[],
  seenExtras: [] as string[],
};
```

## Files

New: `src/lib/settings.svelte.ts`, `src/lib/SettingsPane.svelte`, `src/lib/Toggle.svelte`, `src/lib/menuitems.ts`, `scripts/check-menuids.mjs`.

Touched: `src/lib/store.svelte.ts`, `src/App.svelte`, `src/lib/CommandBar.svelte`, `src/lib/FilePane.svelte`, `src/lib/DrivesPane.svelte`, `src/lib/NavPane.svelte`, `src/lib/TitleBar.svelte`, `src/lib/AddressBar.svelte`, `src/styles/fluent.css`, `package.json`.

No Rust changes are needed for any phase.

## Phase 1 - Settings store, no UI

Create `src/lib/settings.svelte.ts`.

- `export const settings = $state({...DEFAULTS, ...JSON.parse(localStorage.getItem("settings") ?? "{}")})`, read synchronously at module init.
- `export function setSetting<K extends keyof typeof DEFAULTS>(k: K, v: (typeof DEFAULTS)[K])` writes the field and persists the whole object.
- Migration: if the old `theme` or `compact` keys exist in localStorage, fold their values in, persist, and remove the old keys.
- Move `applyTheme()` (the `documentElement.dataset.theme` write) and the `THEMES` list here, and call it once at module init so the no-flash startup is preserved.

Then move the five persisted prefs off `app` and delete the ponytail note at `store.svelte.ts:47`.

- `theme`, `compact`, `showHidden`, `showExt`, `previewPane` leave the `app` state object.
- Read sites to update: `store.svelte.ts:197` (toggleCompact), `250` (displayName), `256` (toggleHidden), `351` and `387` (the two `open_dir` calls), `App.svelte:60` and `129`, `CommandBar.svelte:144-155` and `168`, `FilePane.svelte:54`.
- `toggleHidden` still calls `refresh()`; Alt+P now persists as well as toggles.
- `store.svelte.ts` imports from `settings.svelte.ts` and never the other way round, so there is no import cycle.
- Add `settings` and `setSetting` to the `__e2e` export at `store.svelte.ts:866`.

Done when: `npm run build` and `cargo check` pass, the View menu checkmarks still track their toggles, `localStorage.settings` holds all five fields, the old `theme` and `compact` keys are gone, and a reload restores hidden items and compact view (which it did not do before this phase).

## Phase 2 - Settings page shell and entry point

Route the page.

- `export const SETTINGS = "::Settings"` in `store.svelte.ts`, with a `folderName` case returning "Settings".
- Generalize the virtual branch of `navigate()` at `store.svelte.ts:325` from the hardcoded three-way comparison to `p.startsWith("::")`, matching what `refresh`, `up`, and `setFilter` already do.
  This is the difference between the settings page working and it throwing a directory-listing error.
- `loadVirtual` needs no case: the page has nothing to fetch.
- `AddressBar.svelte:18-20` gets a Settings crumb case.
- `App.svelte:117-136` routes `::Settings` to `SettingsPane`, alongside the existing virtual panes and with no status bar.

Add the entry point.

- A gear button (`&#xE713;`) pinned to the right end of the command bar, `title="Settings"`, navigating to `::Settings`.
  Right-align it with `margin-left: auto` on the button rather than adding a spacer element.
- A "Settings" item at the bottom of the existing "..." menu (`CommandBar.svelte:174-181`), after a separator.

Build the frame.

- `SettingsPane.svelte`: a 220px left rail of categories (Appearance, Files and folders, Context menu, About) over a scrollable content column capped at 720px.
  Selected category is component-local `$state`, so switching is instant and does not touch tab history.
- Match Windows 11 Settings proportions: 36px rail rows with a 16px glyph and a 4px accent selection bar, a 28px semibold page title, section cards using `--card` on a 1px `--stroke` border at 8px radius, 14px semibold section headers, 12px `--text-3` description lines under each control label.
- `Toggle.svelte`: a `<button role="switch" aria-checked>` pill, 40x20, with a knob that slides and takes `--accent` when on.
  It is used roughly thirty times in Phase 5, which is what justifies it being a component.

Done when: the gear opens Settings in the current tab, the tab strip and breadcrumb both read "Settings", Back returns to the previous folder, all four categories render, and the Toggle component switches under both mouse and keyboard.

## Phase 3 - Appearance tab

- Theme picker as a grid of cards, one per entry in `THEMES`.
  Each card is a `<button data-theme={id}>` containing a small mock window built from the same CSS variables the app uses (a title strip in `--card`, a rail in `--surface`, two text bars in `--text-2` and `--text-3`, an accent dot in `--accent`).
  Because `themes.css` selectors are unqualified attribute selectors, setting `data-theme` on the card itself makes it paint in that theme with zero duplicated color values.
  The System card carries no `data-theme` attribute and therefore renders in the current OS mode.
  The active card gets a 2px `--accent` ring and a check glyph.
- Toggles below the grid: Compact view, Preview pane.
- The existing View menu theme flyout stays and calls the same setter, so the two surfaces cannot drift.

Done when: clicking a card applies the theme instantly and survives a reload, every card visibly differs from its neighbors, the active ring follows the selection, and the View menu flyout agrees with the page.

## Phase 4 - Files and folders tab

- Toggles: Hidden items (calls `refresh()`), File name extensions.
- Default view and default sort for new folders: `DEFAULT_PREFS` at `store.svelte.ts:145` stops being a constant and reads `settings.defaultView`, `defaultSort`, `defaultAsc`.
  Per-folder saved prefs still win over these, exactly as today.
- "Clear saved folder view settings": wipes the `folderPrefs` map and its localStorage key, then refreshes.
  This is Explorer's "Reset Folders", and it is also the only way to empty the 500-entry map that currently has no UI.
  Require a second click to confirm, inline on the button, rather than a dialog.
- Start page: Home, This PC, or a typed path, read by `App.svelte:27` and by the new-tab default at `store.svelte.ts:429`.
  A typed path that no longer exists must fall back to Home rather than leaving the app on an error.

Done when: hidden items and extensions survive a reload, a folder with no saved prefs opens in the configured default view and sort, clearing folder prefs makes a previously customized folder fall back to the default, and a restart lands on the configured start page including the invalid-path fallback.

## Phase 5 - Context menu tab, native items

The requested feature.
Every native entry in every right-click menu becomes individually switchable.

Registry, `src/lib/menuitems.ts`:

| Group | Ids |
|---|---|
| File pane, item selected | `file.cut`, `file.copy`, `file.rename`, `file.delete` (the icon strip), `file.open`, `file.openwith`, `file.pin`, `file.copypath`, `file.properties` |
| File pane, blank area | `blank.refresh`, `blank.paste`, `blank.new`, `blank.selectall`, `blank.terminal` |
| This PC drive tile | `drive.open`, `drive.pin`, `drive.properties` |
| Navigation pane pin | `nav.unpin` |
| Tab | `tab.duplicate`, `tab.close`, `tab.closeothers`, `tab.closeright` |

Wiring:

- `menuOn(id)` and `setMenuOn(id, on)` in `settings.svelte.ts`, backed by the `hiddenMenu` array.
- Gate each item at its render site: `FilePane.svelte:927-1048`, `DrivesPane.svelte:112-118`, `NavPane.svelte:166`, `TitleBar.svelte:56-72`.
- The icon strip container renders only if at least one of its four ids is on.
- Separators collapse declaratively, in `fluent.css` next to the existing `.menu-sep` rule, instead of per-site conditions:

```css
.menu-sep:first-child,
.menu-sep:last-child { display: none; }
.menu-sep + .menu-sep { display: none; }
```

  Svelte's block anchors are comment nodes and do not affect `:first-child`, `:last-child`, or `+` matching, but verify it visually anyway with a group fully disabled.
- Empty-menu guard: if a menu would render zero items, do not open it.
  For the file-pane row menu the guard must also account for shell extras still being expected, so check the master toggle from Phase 6 once that lands.
- Settings UI: one card per group, each row a label plus a Toggle, plus a "Reset to defaults" button that empties `hiddenMenu`.

Drift check, `scripts/check-menuids.mjs`, wired as `npm run check:menuids`:
scan `src/lib/*.svelte` for `menuOn("...")`, assert every id found exists in the registry and every registry id is used at least once, exit non-zero otherwise.
This is the one runnable check for the phase and it is what stops later agents from adding a menu item that the settings page never shows.

Done when: unchecking Copy as path removes it from the real right-click menu with no leftover separator and it stays gone after a reload, the drive and tab menus honor their own toggles, disabling an entire group opens no menu at all, and `npm run check:menuids` passes.

## Phase 6 - Context menu tab, shell extension entries

Third-party entries cannot be enumerated ahead of time, so they are learned from use.

- Master toggle `shellExtras`.
  When it is off, `FilePane.onContext` (`FilePane.svelte:424`) and the DrivesPane equivalent skip the `shell_menu` invoke entirely and set `ctxItems = []` immediately, and the warm-up at `App.svelte:30` does not run.
  The visible payoff is no "Loading…" row and no warm shell cost, which is around 1.4s of Windows enumerating handlers per the plan.md notes.
- Seen registry: every `shell_menu` response unions its top-level entry labels, normalized, into `settings.seenExtras`, capped at 200 with the oldest dropped.
  Submenu children are not tracked; hiding a parent hides its flyout with it.
- `hiddenExtras` holds normalized labels; `filterExtras` (`FilePane.svelte:371`) drops them alongside the existing native-verb filtering, and DrivesPane uses the same filter.
- Settings UI: the master toggle, then a scrollable list of every seen entry with its own Toggle, then a "Forget list" button.
  Explain in a description line that the list fills in as context menus are opened, because an empty list on first run otherwise reads as broken.

Done when: with a real third-party entry on this machine (7-Zip or similar), toggling it off makes it disappear from the real right-click menu and stay gone after a reload, the master toggle off yields a menu with only native items and no loading row, and the seen list grows after right-clicking a file and a drive.

## Phase 7 - Polish, reset, verification

- About card: product name, version via `getVersion()` from `@tauri-apps/api/app`, and "Reset all settings" behind an inline confirm that clears the `settings` key and reloads.
- Cross-window sync: a `storage` event listener that re-reads the settings object, so a second window opened with Ctrl+N follows a theme change live.
  Both processes share the WebView2 user-data directory and therefore one browser process, so this should fire; if it does not, leave a `ponytail:` note naming the limit rather than building a polling loop.
- Tab icons for virtual pages: `TitleBar.svelte:85-89` renders the folder icon for every tab, so the Home, This PC, Recycle Bin, and Settings tabs all show a folder.
  Map each virtual path to its own glyph, using the existing `shellIcon(RECYCLE_GUID, 48)` for the bin.
- Escape on the settings page navigates back.
- Full E2E pass on the release build over CDP, one gate at a time from Phases 1 to 6, plus screenshots in two themes for a pixel review of the settings page against Windows 11 Settings.
- Update the status header at the top of `plan.md`, matching the existing format.

Done when: every earlier phase's gate still passes on the release exe, reset returns a heavily customized install to stock, and the settings page survives a side-by-side look at real Settings without anything looking obviously off.

## Traps

Concrete failures that have already bitten this codebase or will bite this feature.

- `navigate()` at `store.svelte.ts:325` hardcodes the three virtual paths while `refresh`, `up`, and `setFilter` test `startsWith("::")`.
  Forgetting Phase 2's generalization sends the settings page through `open_dir`.
- Mutating reactive state during render is illegal in Svelte 5 and fails silently by killing the rest of the component's render, which is exactly how the nav pane once lost its icons.
  Settings reads inside templates are fine; every write belongs in an event handler.
- The theme must be applied at module init from a synchronous localStorage read.
  Moving it behind any await reintroduces a flash of the wrong theme at startup.
- `ShellItem.id` is per-build and must never be persisted.
- `app.showExt` and `app.showHidden` have read sites in both Rust-facing calls and pure display code; missing the `open_dir` calls at `store.svelte.ts:351` and `387` gives a UI toggle that silently does nothing until the next navigation.
- Two Filezy processes share the WebView2 user-data directory, so a dev launch with `--remote-debugging-port` gets no CDP while a release instance is running.
  Kill all instances before an E2E run.
- Icon-related verification is unreliable in dev because CDP module-splitting can fork the module graph and split store state.
  Verify anything icon-shaped on the release build.

## Out of scope

- Custom accent color, custom fonts, or user-authored themes.
  The nine built-in themes cover the request; add a color picker only if it is actually missed.
- Reordering context-menu entries, renaming them, or adding user-defined commands.
  The request was enable and disable.
- Editing real Explorer's own context menu or writing anything to the registry.
- Roaming or syncing settings between machines.
- A settings search box.
  Four categories do not need one.
