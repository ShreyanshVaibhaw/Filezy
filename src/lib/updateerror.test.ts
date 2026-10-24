import { test } from "node:test";
import assert from "node:assert/strict";
import { explainUpdateError } from "./updateerror.ts";

test("update errors read in plain words", () => {
  assert.match(explainUpdateError("Could not fetch a valid release JSON from the remote"), /Couldn't get update information/);
  assert.match(explainUpdateError("error sending request for url (https://github.com/x)"), /Couldn't get update information/);
  assert.equal(explainUpdateError("Download request failed with status: `404 Not Found`"), "The update couldn't be downloaded (the server answered 404). Try again later.");
  assert.match(explainUpdateError("The signature verification failed"), /failed its signature check/);
  assert.equal(explainUpdateError("  something else  "), "something else");
});
