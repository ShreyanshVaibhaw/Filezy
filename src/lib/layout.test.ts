import { test } from "node:test";
import assert from "node:assert/strict";
import { gridStep } from "./layout.ts";

test("Up and Down keep the column across group boundaries", () => {
  // 5 columns; October has 7 items (rows of 5 and 2), September 6.
  const groups = [{ start: 0, count: 7, open: true }, { start: 7, count: 6, open: true }];
  assert.equal(gridStep(6, 1, 5, groups, 13), 8, "row 2 column 2 down to September's column 2");
  assert.equal(gridStep(8, -1, 5, groups, 13), 6, "and back up");
  assert.equal(gridStep(4, 1, 5, groups, 13), 6, "column 5 down to a shorter row takes its last item");
  assert.equal(gridStep(2, -1, 5, groups, 13), 2, "the top row stays");
  assert.equal(gridStep(12, 1, 5, groups, 13), 12, "the bottom row stays");
  assert.equal(gridStep(0, 3, 5, groups, 13), 12, "a page down stops at the last row");
});

test("collapsed groups are skipped, ungrouped grids step by whole rows", () => {
  const groups = [{ start: 0, count: 3, open: true }, { start: 3, count: 4, open: false }, { start: 7, count: 3, open: true }];
  assert.equal(gridStep(1, 1, 4, groups, 10), 8, "over the collapsed group");
  assert.equal(gridStep(4, 1, 4, groups, 10), 7, "from inside a collapsed group, down");
  assert.equal(gridStep(4, -1, 4, groups, 10), 0, "and up");
  assert.equal(gridStep(3, 1, 4, null, 10), 7);
  assert.equal(gridStep(9, -2, 4, null, 10), 1);
});
