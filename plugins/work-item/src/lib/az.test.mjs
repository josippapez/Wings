import assert from "node:assert/strict";
import test from "node:test";

const calls = [];
globalThis.wings = {
  exec: async (program, args) => {
    calls.push(args);
    const ids = new URL(args[args.indexOf("--url") + 1]).searchParams.get("ids").split(",").map(Number);
    return { code: 0, stdout: JSON.stringify({ count: ids.length, value: ids.map((id) => (id === 3 ? null : { id, fields: { "System.Title": `T${id}` }, relations: [] })) }), stderr: "" };
  },
};
const { workItem } = await import("./az.ts");

// `az devops invoke --resource workItems` fails on azure-cli 2.91 with `KeyError: 'type'`, so the items are read through `az rest`.
test("work items asked in one turn are one az rest, and one that isn't there is null", async () => {
  const org = "https://dev.azure.com/contoso";
  const [a, b, c] = await Promise.all([workItem(org, 1), workItem(org, "2"), workItem(org, 3)]);
  assert.deepEqual([a.fields["System.Title"], b.id, c], ["T1", 2, null]);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "rest");
  assert.equal(calls[0].includes("invoke"), false);
});
