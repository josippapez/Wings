// Test-only deterministic key. Never used by the app or release scripts.
import { createPrivateKey, createPublicKey, createHash, sign } from "node:crypto";
import { readFileSync } from "node:fs";
const seed = Buffer.alloc(32, Number(process.argv[2] || 1));
const key = createPrivateKey({ key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]), format: "der", type: "pkcs8" });
const publicBytes = createPublicKey(key).export({ format: "der", type: "spki" }).subarray(-32);
const id = Buffer.alloc(8, 42);
const data = readFileSync(0);
const sig = sign(null, createHash("blake2b512").update(data).digest(), key);
const comment = "test fixture";
const signature = `untrusted comment: test only\n${Buffer.concat([Buffer.from("ED"), id, sig]).toString("base64")}\ntrusted comment: ${comment}\n${sign(null, Buffer.concat([sig, Buffer.from(comment)]), key).toString("base64")}\n`;
const publicKey = `untrusted comment: test only\n${Buffer.concat([Buffer.from("Ed"), id, publicBytes]).toString("base64")}\n`;
console.log(JSON.stringify({ key: Buffer.from(publicKey).toString("base64"), signature: Buffer.from(signature).toString("base64") }));
