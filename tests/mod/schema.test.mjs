// The schema checker must itself reject what it should, or the status test above proves nothing.
import { test } from "node:test";
import assert from "node:assert/strict";
import { validate } from "./schema.mjs";

const schema = {
  type: "object",
  additionalProperties: false,
  required: ["version", "list"],
  properties: {
    version: { const: 1 },
    list: { type: "array", items: { $ref: "#/$defs/item" } },
    note: { type: ["string", "null"] },
  },
  $defs: { item: { type: "object", required: ["s"], properties: { s: { enum: ["a", "b"] }, n: { type: "integer", minimum: 1 } } } },
};

test("accepts a valid document", () => {
  assert.deepEqual(validate(schema, { version: 1, list: [{ s: "a", n: 2 }], note: null }), []);
});

test("rejects each kind of mistake", () => {
  assert.notDeepEqual(validate(schema, { version: 2, list: [] }), []);
  assert.notDeepEqual(validate(schema, { version: 1 }), []);
  assert.notDeepEqual(validate(schema, { version: 1, list: [], extra: 1 }), []);
  assert.notDeepEqual(validate(schema, { version: 1, list: [{ s: "c" }] }), []);
  assert.notDeepEqual(validate(schema, { version: 1, list: [{ s: "a", n: 0 }] }), []);
  assert.notDeepEqual(validate(schema, { version: 1, list: [{ s: "a", n: 1.5 }] }), []);
  assert.notDeepEqual(validate(schema, { version: 1, list: [{}] }), []);
  assert.notDeepEqual(validate(schema, { version: 1, list: [], note: 3 }), []);
});

test("an unknown keyword throws rather than passing", () => {
  assert.throws(() => validate({ type: "object", format: "x" }, {}), /unsupported schema keyword: format/);
});
