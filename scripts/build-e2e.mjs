// Builds the exe the E2E runner drives (the `e2e` feature) and the stand-in
// cloud provider the cloud suite starts. Both go to src-tauri/target-e2e, so
// src-tauri/target/release/filezy.exe stays a real release build and switching
// between the two doesn't redo the release link.
//
//   npm run build:e2e                optimized exe: target-e2e/release/filezy.exe
//   npm run build:e2e -- --debug     faster to build: target-e2e/debug/filezy.exe
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const E2E_TARGET = path.join(repo, "src-tauri", "target-e2e");

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const debug = process.argv.includes("--debug");
  const env = { ...process.env, CARGO_TARGET_DIR: E2E_TARGET };
  const run = (cmd, args) => {
    const r = spawnSync(cmd, args, { cwd: repo, env, stdio: "inherit" });
    if (r.status !== 0) process.exit(r.status ?? 1);
  };
  const tauri = path.join(repo, "node_modules", "@tauri-apps", "cli", "tauri.js");
  run(process.execPath, [tauri, "build", "--no-bundle", "--features", "e2e", ...(debug ? ["--debug"] : [])]);
  run("cargo", ["build", "--locked", "--example", "testcloud", "--manifest-path", path.join("src-tauri", "Cargo.toml")]);
  console.log(`built ${path.join(E2E_TARGET, debug ? "debug" : "release", "filezy.exe")}`);
}
