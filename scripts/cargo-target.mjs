// Where Cargo writes this repo's builds. It's src-tauri/target unless a Cargo
// config (`build.target-dir`) or CARGO_TARGET_DIR moves it, so ask Cargo rather
// than assume.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const manifest = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "Cargo.toml");

let cached;
export function cargoTarget() {
  cached ??= JSON.parse(execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps", "--manifest-path", manifest],
    { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] })).target_directory;
  return cached;
}
