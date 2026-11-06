// The fixture plugin's main: a count kept in storage, served as a query, and an ask that adds one to it.
import { ch } from "./messages";

const KEY = "count";
const read = async () => ((await wings.storage.get(KEY)) as number | null) ?? 0;

ch.serve("count", read);
ch.answer("increment", async () => {
  const next = (await read()) + 1;
  await wings.storage.set(KEY, next);
  ch.put("count", undefined, next);
  return next;
});
ch.answer("echo", (text) => text.length);
