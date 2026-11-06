import assert from "node:assert/strict";
import { test } from "node:test";

test("native commit dates do not need Intl", async () => {
  globalThis.WINGS_NATIVE = true;
  const intl = globalThis.Intl;
  try {
    globalThis.Intl = undefined;
    const { dateOf, dateTimeOf } = await import("./dates.ts");
    assert.equal(dateOf(0), "1970-01-01");
    assert.equal(dateTimeOf(0), "1970-01-01T00:00:00.000Z");
  } finally {
    globalThis.Intl = intl;
    delete globalThis.WINGS_NATIVE;
  }
});
