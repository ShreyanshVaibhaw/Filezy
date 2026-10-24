# Repo rules

- Never commit or push planning Markdown: blueprints, plans, audits, `remaining-from-plans.md` and anything like them.
  `.gitignore` covers the known names; check `git status` before every commit anyway.
- Run `npm run verify` before committing.
  It runs the menu-id check, svelte-check, the node tests, the frontend build and the Rust tests.
- CI also runs `cargo fmt --check` and clippy with `-D warnings`, with and without `--features e2e`.
  Run them locally when you touch Rust.
- `npm run build:e2e` builds the test exe into `<target>-e2e` next to Cargo's build folder, `src-tauri/target-e2e` by default (add `-- --debug` for a faster debug build).
  It never replaces the release `filezy.exe`, which must stay a real release build.
- Scripts find Cargo's build folder with `scripts/cargo-target.mjs`, so a global `build.target-dir` or `CARGO_TARGET_DIR` works; never hard-code `src-tauri/target`.
- `npm run test:e2e` drives that exe through WebView2's DevTools protocol, against a throwaway profile under `%TEMP%`.
  Pass `-- --debug` to use the debug exe and a name fragment to run only matching tests.
- Some E2E tests take real input: keyboard tests bring the app window to the front, and clipboard tests replace the clipboard.
  Run those only when nobody is using the machine.
- For checks in the running app while someone is using the machine, use ExtraCursor with background dispatch and a disposable profile (`WEBVIEW2_USER_DATA_FOLDER` and `APPDATA` in a scratch folder).
  Never drive the installed Filezy or its data.
