import { test } from "node:test";
import assert from "node:assert/strict";
import { filterOf, tokens, withFilter, withoutFilters } from "./searchquery.ts";

test("tokens keep phrases and two-word dates together", () => {
  assert.deepEqual(tokens('"annual report" datemodified:this week kind:document'), [
    '"annual report"',
    "datemodified:this week",
    "kind:document",
  ]);
});

test("setting a filter replaces it under any of its names", () => {
  assert.equal(withFilter("report kind:music", "kind", "document"), "report kind:document");
  assert.equal(withFilter("report modified:today", "datemodified", "this week"), "report datemodified:this week");
  assert.equal(withFilter("report type:.pdf", "ext", null), "report");
  assert.equal(withFilter("", "size", "large"), "size:large");
});

test("reading filters back, and clearing them all", () => {
  const q = "plan datemodified:last month size:>1MB kind:=Picture";
  assert.equal(filterOf(q, "datemodified"), "last month");
  assert.equal(filterOf(q, "kind"), "picture");
  assert.equal(filterOf(q, "datecreated"), null);
  assert.equal(withoutFilters(q), "plan");
});
