import { test } from "node:test";
import assert from "node:assert/strict";
import { baseName, crumbs, displayPath, isArchiveName, isLibrary, isServer, isUnc, isWithin, isWsl, pathKey, joinPath, parentPath, rootOf, samePath } from "./paths.ts";

test("archive locations", () => {
  const root = "C:\\x\\a.zip|";
  assert.equal(joinPath(root, "docs"), "C:\\x\\a.zip|docs");
  assert.equal(joinPath("C:\\x\\a.zip|docs", "f.txt"), "C:\\x\\a.zip|docs\\f.txt");
  assert.equal(parentPath("C:\\x\\a.zip|docs\\sub"), "C:\\x\\a.zip|docs");
  assert.equal(parentPath("C:\\x\\a.zip|docs"), root);
  assert.equal(parentPath(root), "C:\\x");
  assert.equal(baseName(root), "a.zip");
  assert.equal(baseName("C:\\x\\a.zip|docs\\sub"), "sub");
  assert.equal(displayPath("C:\\x\\a.zip|docs\\sub"), "C:\\x\\a.zip\\docs\\sub");
  assert.equal(displayPath(root), "C:\\x\\a.zip");
  assert.deepEqual(crumbs("C:\\x\\a.zip|docs"), [
    { name: "C:", path: "C:\\" },
    { name: "x", path: "C:\\x" },
    { name: "a.zip", path: root },
    { name: "docs", path: "C:\\x\\a.zip|docs" },
  ]);
  assert.equal(isWithin("C:\\x\\a.zip|docs\\f.txt", root), true);
  assert.equal(isWithin("C:\\x\\a.zip|docs", "C:\\x\\a.zip|doc"), false);
  assert.equal(isArchiveName("Backup.TAR.GZ"), true);
  assert.equal(isArchiveName("report.docx"), false);
});

test("parents of drive paths", () => {
  assert.equal(parentPath("C:\\a\\b"), "C:\\a");
  assert.equal(parentPath("C:\\a"), "C:\\");
  assert.equal(parentPath("C:\\a\\"), "C:\\");
  assert.equal(parentPath("C:\\"), null);
  assert.equal(parentPath("::Home"), null);
});

test("parents of UNC paths", () => {
  assert.equal(parentPath("\\\\srv\\sh\\a\\b"), "\\\\srv\\sh\\a");
  assert.equal(parentPath("\\\\srv\\sh\\a"), "\\\\srv\\sh");
  assert.equal(parentPath("\\\\srv\\sh"), "\\\\srv");
  assert.equal(parentPath("\\\\srv"), null);
});

test("a computer on its own", () => {
  assert.equal(isServer("\\\\srv"), true);
  assert.equal(isServer("\\\\srv\\"), true);
  assert.equal(isServer("\\\\srv\\share"), false);
  assert.equal(isServer("C:\\"), false);
});

test("roots", () => {
  assert.equal(rootOf("C:\\x\\y"), "C:\\");
  assert.equal(rootOf("d:"), "d:\\");
  assert.equal(rootOf("\\\\srv\\sh\\x"), "\\\\srv\\sh\\");
  assert.equal(rootOf("\\\\srv"), null);
  assert.equal(isUnc("\\\\?\\C:\\x"), false);
  assert.equal(isUnc("\\\\srv"), true);
});

test("names and joins", () => {
  assert.equal(joinPath("C:\\", "a"), "C:\\a");
  assert.equal(joinPath("C:\\a\\", "b"), "C:\\a\\b");
  assert.equal(baseName("C:\\a\\b.txt"), "b.txt");
  assert.equal(baseName("C:\\"), "C:\\");
  assert.equal(baseName("\\\\srv\\sh"), "sh");
});

test("breadcrumbs", () => {
  assert.deepEqual(crumbs("C:\\a\\b"), [
    { name: "C:", path: "C:\\" },
    { name: "a", path: "C:\\a" },
    { name: "b", path: "C:\\a\\b" },
  ]);
  assert.deepEqual(crumbs("\\\\srv\\sh\\a"), [
    { name: "srv", path: "\\\\srv" },
    { name: "sh", path: "\\\\srv\\sh" },
    { name: "a", path: "\\\\srv\\sh\\a" },
  ]);
});

test("comparisons", () => {
  assert.ok(samePath("C:\\Users\\", "c:\\users"));
  assert.ok(isWithin("C:\\a\\b", "C:\\a"));
  assert.ok(isWithin("C:\\a", "c:\\a\\"));
  assert.ok(!isWithin("C:\\ab", "C:\\a"));
});

test("libraries", () => {
  const lib = "C:\\Users\\me\\AppData\\Roaming\\Microsoft\\Windows\\Libraries\\Documents.library-ms";
  assert.ok(isLibrary(lib) && isLibrary("C:\\x\\Music.LIBRARY-MS") && !isLibrary("C:\\x\\library-ms"));
  assert.equal(baseName(lib), "Documents");
  assert.deepEqual(crumbs(lib), [
    { name: "Libraries", path: "C:\\Users\\me\\AppData\\Roaming\\Microsoft\\Windows\\Libraries" },
    { name: "Documents", path: lib },
  ]);
});

test("WSL mount roots and case-sensitive Linux paths", () => {
  const root = "\\\\wsl.localhost\\Ubuntu";
  assert.ok(isWsl(root) && isWsl("\\\\wsl$\\Ubuntu\\home"));
  assert.equal(parentPath(root), null);
  assert.equal(parentPath(root + "\\home"), root);
  assert.ok(samePath(root + "\\home", "\\\\WSL.LOCALHOST\\ubuntu\\home"));
  assert.ok(!samePath(root + "\\Home", root + "\\home"));
  assert.ok(!isWithin(root + "\\Home\\file", root + "\\home"));
  assert.equal(pathKey(root + "\\Home\\"), "\\\\wsl.localhost\\ubuntu\\Home");
  assert.deepEqual(crumbs(root + "\\home"), [{ name: "Ubuntu", path: root }, { name: "home", path: root + "\\home" }]);
});
