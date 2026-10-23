// Match the installer, Minisign signature and release version before publishing,
// and refuse a working tree that isn't the clean, tagged release commit.
import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash, createPublicKey, verify } from "node:crypto";
import { execFileSync } from "node:child_process";

export function verifySignature(bytes, signature, publicKey) {
  const decode = (value) => {
    if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) throw new Error("Installer signature has an invalid format.");
    return Buffer.from(value, "base64");
  };
  const keyLines = decode(publicKey.trim()).toString("utf8").trim().split(/\r?\n/);
  const pub = decode(keyLines[1] ?? "");
  const lines = decode(signature.trim()).toString("utf8").trim().split(/\r?\n/);
  const packet = decode(lines[1] ?? "");
  if (pub.length !== 42 || pub.subarray(0, 2).toString() !== "Ed" || packet.length !== 74 || lines.length !== 4 || !lines[0]?.startsWith("untrusted comment:") ||
      !lines[2]?.startsWith("trusted comment: ") || !pub.subarray(2, 10).equals(packet.subarray(2, 10))) {
    throw new Error("Installer signature has an invalid format or a different signing key.");
  }
  const alg = packet.subarray(0, 2).toString();
  if (!["Ed", "ED"].includes(alg)) throw new Error("Unsupported signature algorithm.");
  const edKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), pub.subarray(10)]), format: "der", type: "spki" });
  const signed = alg === "ED" ? createHash("blake2b512").update(bytes).digest() : bytes;
  const sig = packet.subarray(10);
  if (!verify(null, signed, edKey, sig) ||
      !verify(null, Buffer.concat([sig, Buffer.from(lines[2].slice(17))]), edKey, decode(lines[3] ?? ""))) {
    throw new Error("Installer signature verification failed. Sign this exact installer.");
  }
}

export function validateInstaller(bytes) {
  if (bytes.length < 64 || bytes.toString("ascii", 0, 2) !== "MZ") throw new Error("Installer is not a Windows executable.");
  const pe = bytes.readUInt32LE(60);
  if (pe + 6 > bytes.length || bytes.toString("ascii", pe, pe + 4) !== "PE\0\0") throw new Error("Invalid PE header.");
  // NSIS bootstraps are 32-bit even for a 64-bit payload. The target is taken
  // from Tauri's exact filename, rather than inferred from the bootstrap.
}

export function makeManifest({ dir, version, publicKey, repo, arch = "x86_64", notes = `Filezy ${version}` }) {
  if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(version)) throw new Error("Invalid release version.");
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) throw new Error("Invalid release repository.");
  const target = { x86_64: "x64", aarch64: "arm64" }[arch];
  if (!target) throw new Error(`Unsupported updater architecture: ${arch}`);
  const expected = `Filezy_${version}_${target}-setup.exe`;
  const installers = readdirSync(dir).filter((name) => name.endsWith("-setup.exe"));
  if (installers.length !== 1 || installers[0] !== expected) {
    throw new Error(`Expected only ${expected} in ${dir}; found ${installers.join(", ") || "none"}. Use a clean bundle directory.`);
  }
  const bytes = readFileSync(join(dir, expected));
  validateInstaller(bytes);
  const signature = readFileSync(join(dir, `${expected}.sig`), "utf8").trim();
  verifySignature(bytes, signature, publicKey);
  return {
    version, notes, pub_date: new Date().toISOString().replace(/\.\d+Z$/, "Z"),
    platforms: { [`windows-${arch}`]: { signature,
      url: `https://github.com/${repo}/releases/download/v${version}/${encodeURIComponent(expected)}`,
    } },
  };
}

/// A release is built from exactly what its tag names: no uncommitted or
/// untracked files, and HEAD tagged v<version>.
export function checkReleaseTree(version, cwd = ".") {
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  const dirty = git("status", "--porcelain").trim();
  if (dirty) throw new Error(`The working tree has uncommitted changes; release from a clean checkout:\n${dirty}`);
  const tags = git("tag", "--points-at", "HEAD").split(/\r?\n/).filter(Boolean);
  if (!tags.includes(`v${version}`)) {
    throw new Error(`HEAD is not tagged v${version}${tags.length ? ` (it has ${tags.join(", ")})` : ""}; tag the release commit first.`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const conf = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
    const pkg = JSON.parse(readFileSync("package.json", "utf8"));
    const cargo = readFileSync("src-tauri/Cargo.toml", "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
    if (pkg.version !== conf.version || cargo !== conf.version) throw new Error("Package, Cargo and Tauri versions must match.");
    checkReleaseTree(conf.version);
    const manifest = makeManifest({ dir: process.env.FILEZY_BUNDLE_DIR ?? "src-tauri/target/release/bundle/nsis",
      version: conf.version, publicKey: conf.plugins.updater.pubkey, repo: process.env.FILEZY_REPO ?? "ShreyanshVaibhaw/Filezy",
      arch: process.env.FILEZY_ARCH ?? "x86_64", notes: process.argv[2] });
    writeFileSync("latest.json", JSON.stringify(manifest, null, 2) + "\n");
    console.log(`latest.json verified and written for v${conf.version}`);
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
