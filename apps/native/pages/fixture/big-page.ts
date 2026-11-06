// A bare page for the host's tests: an event with `{ "size": n }` asks main to count a text of n characters, which is
// too big for one broadcast when n is large, and the page shows the answer.
import { ch } from "./messages";

const show = (text: string) => wings.render(JSON.stringify({ id: 0, type: "#root", props: {}, children: [{ id: 1, type: "#text", text }] }));

show("ready");
(globalThis as { __wingsEvent?: unknown }).__wingsEvent = (_handler: string, payload: string) => {
  const { size } = JSON.parse(payload) as { size: number };
  ch.ask("echo", "é😀".repeat(size / 3)).then(
    (length) => show(`echo ${length}`),
    (error: Error) => show(`failed ${error.message}`),
  );
  return true;
};
