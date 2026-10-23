import { test } from "node:test";
import assert from "node:assert/strict";
import { parseSaved } from "./savedparse.ts";

test("saved searches and workspaces keep only well-formed entries", () => {
  const r = parseSaved(JSON.stringify({
    searches: [{ name: "Big", root: "C:\\x", query: "size:>1MB", scope: "subfolders" }, { name: "", root: "C:\\", query: "a", scope: "folder" }, { name: "x", root: "C:\\", query: "a", scope: "moon" }],
    workspaces: [{ name: "Work", tabs: ["C:\\a", "::Home"] }, { name: "Empty", tabs: [] }, { name: "Bad", tabs: [1] }],
  }));
  assert.deepEqual(r.searches.map((s) => s.name), ["Big"]);
  assert.deepEqual(r.workspaces.map((w) => w.name), ["Work"]);
  assert.deepEqual(parseSaved("{not json"), { searches: [], workspaces: [] });
  assert.deepEqual(parseSaved(null), { searches: [], workspaces: [] });
});

test("a name saved twice keeps only its first entry", () => {
  const r = parseSaved(JSON.stringify({
    searches: [{ name: "Big", root: "C:\a", query: "a", scope: "folder" }, { name: "big", root: "C:\b", query: "b", scope: "folder" }],
    workspaces: [{ name: "W", tabs: ["C:\a"] }, { name: "W", tabs: ["C:\b"] }],
  }));
  assert.deepEqual(r.searches.map((s) => s.root), ["C:\a"]);
  assert.deepEqual(r.workspaces.map((w) => w.tabs[0]), ["C:\a"]);
});
