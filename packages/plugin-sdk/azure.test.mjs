import assert from "node:assert/strict";
import { test } from "node:test";

import { cli } from "./cli.js";
import { workItems } from "./azure.js";

const AZ = { signedOut: /login/i, signIn: [] };

test("workItems asks for every id of a turn in one az rest, 200 at most, and gives null for an id that isn't there", async () => {
  const calls = [];
  const wings = {
    exec: async (program, args) => {
      calls.push({ program, args });
      const ids = new URL(args[args.indexOf("--url") + 1]).searchParams.get("ids").split(",").map(Number);
      return { code: 0, stdout: JSON.stringify({ count: ids.length, value: ids.map((id) => (id === 7 ? null : { id, fields: {}, relations: [] })) }), stderr: "" };
    },
  };
  const items = workItems(cli("az", { ...AZ, wings }), "https://contoso.visualstudio.com");
  const ids = Array.from({ length: 201 }, (_, i) => i + 1);
  const got = await Promise.all(ids.map((id) => items.get(id)));
  assert.deepEqual([got[0].id, got[6], got[200].id], [1, null, 201]);
  assert.equal(calls.length, 2, "201 ids are two calls");
  const url = calls[0].args[calls[0].args.indexOf("--url") + 1];
  assert.match(url, /^https:\/\/dev\.azure\.com\/contoso\/_apis\/wit\/workitems\?ids=1,2,3.*&\$expand=relations&errorPolicy=omit&api-version=7\.1$/);
  assert.deepEqual(calls[0].args.slice(0, 3), ["rest", "--resource", "499b84ac-1321-427f-aa17-267ca6975798"]);
  assert.equal(calls[0].args.includes("--method"), false, "az rest reads with GET by default, and Wings refuses --method");
});
