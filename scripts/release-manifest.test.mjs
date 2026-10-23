import { test } from "node:test";
import assert from "node:assert/strict";
import { generateKeyPairSync, sign, createHash } from "node:crypto";
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { checkReleaseTree, makeManifest, verifySignature } from "./make-latest-json.mjs";

function fixture() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const id = Buffer.from("12345678");
  const pub = Buffer.concat([Buffer.from("Ed"), id, publicKey.export({ format: "der", type: "spki" }).subarray(-32)]);
  const key = Buffer.from(`untrusted comment: test\n${pub.toString("base64")}\n`).toString("base64");
  const bytes = Buffer.alloc(128);
  bytes.write("MZ"); bytes.writeUInt32LE(64, 60); bytes.write("PE\0\0", 64); bytes.writeUInt16LE(0x14c, 68);
  const signature = (data) => {
    const sig = sign(null, createHash("blake2b512").update(data).digest(), privateKey);
    const packet = Buffer.concat([Buffer.from("ED"), id, sig]);
    const global = sign(null, Buffer.concat([sig, Buffer.from("test")]), privateKey);
    return Buffer.from(`untrusted comment: test\n${packet.toString("base64")}\ntrusted comment: test\n${global.toString("base64")}\n`).toString("base64");
  };
  return { key, bytes, signature };
}

test("manifest accepts the exact signed installer and rejects stale, changed or ambiguous bundles", () => {
  const dir = mkdtempSync(join(tmpdir(), "filezy-manifest-"));
  try {
    const f = fixture(); const name = "Filezy_0.1.0_x64-setup.exe";
    writeFileSync(join(dir, name), f.bytes);
    writeFileSync(join(dir, `${name}.sig`), f.signature(f.bytes));
    const options = { dir, version: "0.1.0", publicKey: f.key, repo: "owner/repo" };
    assert.match(makeManifest(options).platforms["windows-x86_64"].url, /Filezy_0.1.0_x64-setup.exe$/);
    assert.throws(() => makeManifest({ ...options, version: "0.2.0" }), /Expected only/);
    assert.throws(() => makeManifest({ ...options, arch: "aarch64" }), /Expected only/);
    const altered = Buffer.from(f.bytes); altered[100] = 1;
    writeFileSync(join(dir, name), altered);
    assert.throws(() => makeManifest(options), /verification failed/);
    writeFileSync(join(dir, "Filezy_0.0.9_x64-setup.exe"), f.bytes);
    assert.throws(() => makeManifest(options), /Expected only/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("release tree must be clean and HEAD tagged with the version", () => {
  const dir = mkdtempSync(join(tmpdir(), "filezy-release-tree-"));
  try {
    const git = (...args) => execFileSync("git", ["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", ...args], { cwd: dir, stdio: "pipe" });
    git("init", "-q");
    writeFileSync(join(dir, "a.txt"), "a");
    git("add", "a.txt");
    git("commit", "-q", "-m", "first");
    assert.throws(() => checkReleaseTree("0.1.0", dir), /not tagged v0\.1\.0/);
    git("tag", "v0.0.9");
    assert.throws(() => checkReleaseTree("0.1.0", dir), /it has v0\.0\.9/);
    git("tag", "v0.1.0");
    checkReleaseTree("0.1.0", dir);
    writeFileSync(join(dir, "a.txt"), "changed");
    assert.throws(() => checkReleaseTree("0.1.0", dir), /uncommitted changes/);
    git("checkout", "-q", "--", "a.txt");
    writeFileSync(join(dir, "new.txt"), "untracked");
    assert.throws(() => checkReleaseTree("0.1.0", dir), /uncommitted changes/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("signature rejects a wrong key and a changed trusted comment", () => {
  const f = fixture(); const sig = f.signature(f.bytes);
  assert.throws(() => verifySignature(f.bytes, sig, fixture().key), /verification failed/);
  const altered = Buffer.from(Buffer.from(sig, "base64").toString().replace("\ntrusted comment: test\n", "\ntrusted comment: altered\n")).toString("base64");
  assert.throws(() => verifySignature(f.bytes, altered, f.key));
  assert.throws(() => verifySignature(f.bytes, "", f.key), /invalid format/);
  assert.throws(() => verifySignature(f.bytes, sig + "!", f.key), /invalid format/);
});

test("manifest verifies a signature produced by Tauri's actual signer", () => {
  const dir = mkdtempSync(join(tmpdir(), "filezy-tauri-signer-"));
  try {
    const key = join(dir, "test.key");
    const installer = join(dir, "Filezy_0.1.0_x64-setup.exe");
    writeFileSync(installer, fixture().bytes);
    const cli = resolve("node_modules/@tauri-apps/cli/tauri.js");
    const run = (args) => {
      try { execFileSync(process.execPath, [cli, "signer", ...args], { stdio: "pipe", timeout: 60_000 }); }
      catch (error) { throw new Error(`Tauri test signer failed with status ${error.status ?? "unknown"}`); }
    };
    run(["generate", "--ci", "--password", "test-only-password", "--write-keys", key]);
    run(["sign", "--password", "test-only-password", "--private-key-path", key, installer]);
    const manifest = makeManifest({ dir, version: "0.1.0", publicKey: readFileSync(key + ".pub", "utf8").trim(), repo: "test/repo" });
    assert.ok(manifest.platforms["windows-x86_64"].signature.length > 100);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
