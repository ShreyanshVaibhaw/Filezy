import { test } from "node:test";
import assert from "node:assert/strict";
import { translate, pluralMessage } from "./messages.ts";

test("messages fall back, interpolate by name and leave missing placeholders intact", () => {
  assert.equal(translate("Open {name}", { name: "a.txt" }), "Open a.txt");
  assert.equal(translate("Open {name}", {}, { "Open {name}": "Ouvrir {name}" }), "Ouvrir {name}");
  assert.equal(translate("Open {name}", { name: "<a>" }, {}, true), "[OOpeen <a>]");
  assert.equal(pluralMessage(1, "one", "other"), "one");
  assert.equal(pluralMessage(2, "one", "other"), "other");
});
