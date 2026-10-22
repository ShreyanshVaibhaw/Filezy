import { test } from "node:test";
import assert from "node:assert/strict";
import { ruleNames, type RenameRule } from "./renamerules.ts";

const base: RenameRule = { mode: "number", base: "Trip", find: "", replace: "", regex: false, matchCase: false, template: "", start: 1 };
const items = [
  { path: "C:\\Photos\\IMG_001.JPG", is_dir: false, modified: new Date(2026, 9, 3).getTime() },
  { path: "C:\\Photos\\img_002.jpg", is_dir: false, modified: new Date(2026, 9, 4).getTime() },
  { path: "C:\\Photos\\Album.v2", is_dir: true },
];

test("numbered names keep each extension, folders keep theirs whole", () => {
  assert.deepEqual(ruleNames(base, items).names, ["Trip (1).JPG", "Trip (2).jpg", "Trip (3)"]);
});

test("find and replace: plain, case, regex, and bad patterns", () => {
  const r = { ...base, mode: "replace" as const, find: "img_", replace: "Rome " };
  assert.deepEqual(ruleNames(r, items).names, ["Rome 001.JPG", "Rome 002.jpg", "Album.v2"]);
  assert.deepEqual(ruleNames({ ...r, matchCase: true }, items).names.slice(0, 2), ["IMG_001.JPG", "Rome 002.jpg"]);
  assert.deepEqual(ruleNames({ ...r, regex: true, find: "^img_0*(\\d+)$", replace: "#$1" }, items).names.slice(0, 2), ["#1.JPG", "#2.jpg"]);
  assert.deepEqual(ruleNames({ ...r, find: "." }, items.slice(2)).names, ["AlbumRome v2"], "a plain dot matches only dots");
  assert.match(ruleNames({ ...r, regex: true, find: "(" }, items).error, /isn't valid/);
  assert.match(ruleNames({ ...r, find: "" }, items).error, /find/);
  assert.match(ruleNames({ ...r, replace: "a/b" }, items).error, /characters/);
});

test("templates fill tokens and catch duplicates", () => {
  const t = { ...base, mode: "template" as const, template: "{parent} {date} {n:3}", start: 9 };
  assert.deepEqual(ruleNames(t, items.slice(0, 2)).names, ["Photos 2026-10-03 009.JPG", "Photos 2026-10-04 010.jpg"]);
  assert.match(ruleNames({ ...t, template: "same" }, items.slice(0, 2)).error, /same name/);
  assert.match(ruleNames({ ...t, template: "" }, items.slice(0, 1)).error, /usable/);
});
