// Builds the update manifest the app fetches from GitHub.
// Run after `npm run tauri build` (which must have been signed), then attach
// both the installer and latest.json to the release:
//
//   npm run release:manifest
//   gh release create v0.1.0 <installer> latest.json
//
// The app's endpoint is .../releases/latest/download/latest.json, so every new
// release only has to carry its own manifest.
import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const conf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const version = conf.version;
const repo = process.env.FILEZY_REPO ?? "ShreyanshVaibhaw/Filezy";

const dir = "src-tauri/target/release/bundle/nsis";
const files = readdirSync(dir);
const setup = files.find((f) => f.endsWith("-setup.exe"));
const sig = files.find((f) => f.endsWith("-setup.exe.sig"));
if (!setup || !sig) {
  console.error(`no signed installer in ${dir}. Build with TAURI_SIGNING_PRIVATE_KEY_PATH set.`);
  console.error("found: " + files.join(", "));
  process.exit(1);
}

const manifest = {
  version,
  notes: process.argv[2] ?? `Filezy ${version}`,
  pub_date: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
  platforms: {
    "windows-x86_64": {
      signature: readFileSync(join(dir, sig), "utf8").trim(),
      url: `https://github.com/${repo}/releases/download/v${version}/${setup}`,
    },
  },
};

writeFileSync("latest.json", JSON.stringify(manifest, null, 2) + "\n");
console.log(`latest.json written for v${version}`);
console.log(`installer: ${join(dir, setup)}`);
