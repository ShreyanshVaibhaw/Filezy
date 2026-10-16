import { test } from "node:test";
import assert from "node:assert/strict";
import { formatWith } from "./datefmt.ts";

const f = { date: "", time: "", am: "AM", pm: "PM" };
const d = new Date(2026, 9, 2, 5, 7, 9); // 2 October 2026, 05:07:09

test("day, month and year patterns", () => {
  assert.equal(formatWith("dd-MM-yyyy", d, f), "02-10-2026");
  assert.equal(formatWith("M/d/yyyy", d, f), "10/2/2026");
  assert.equal(formatWith("yyyy-MM-dd", d, f), "2026-10-02");
  assert.equal(formatWith("d.M.yy", d, f), "2.10.26");
});

test("time patterns, 24 and 12 hour", () => {
  assert.equal(formatWith("HH:mm", d, f), "05:07");
  assert.equal(formatWith("H:mm:ss", d, f), "5:07:09");
  assert.equal(formatWith("h:mm tt", d, f), "5:07 AM");
  assert.equal(formatWith("hh:mm t", new Date(2026, 9, 2, 17, 7), f), "05:07 P");
});

test("quoted text is literal", () => {
  assert.equal(formatWith("yyyy'年'M'月'd'日'", d, f), "2026年10月2日");
});
