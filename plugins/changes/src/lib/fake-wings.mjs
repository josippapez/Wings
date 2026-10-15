// A `wings` for the tests that runs no git. Install it once per test file: the SDK's channel keeps the first `onBroadcast`
// it is given. Each test sets `fake.reply(args, cwd)`, what `git <args>` prints in `cwd` (a result, or an Error for a call
// Wings refuses), and finds what was broadcast in `fake.sent`.

export const ok = (stdout = "") => ({ code: 0, stdout, stderr: "" });
export const no = { code: 128, stdout: "", stderr: "fatal: not a git repository" };

/** A git where every folder is a repo on branch main, unless it is in `plain`. */
export const repos =
  (...plain) =>
  (args, cwd) => {
    if (args === "rev-parse --show-toplevel") return plain.includes(cwd) ? no : ok(`${cwd}\n`);
    if (args === "rev-parse --verify --quiet HEAD") return ok("abc1234567\n");
    if (args === "branch --show-current") return ok("main\n");
    return ok();
  };

export function fakeWings() {
  let asks = 0;
  const fake = {
    reply: repos(),
    sent: [],
    heard: undefined,
    /** Asks the way a page's channel does, and resolves the answer or rejects with its error. */
    async ask(name, request) {
      const id = `t${++asks}`;
      fake.heard({ "wings:sdk": 1, kind: "ask", id, name, request });
      while (!fake.sent.some((m) => m.id === id)) await new Promise((resolve) => setTimeout(resolve, 1));
      const { error, response } = fake.sent.find((m) => m.id === id);
      if (error !== undefined) throw new Error(error);
      return response;
    },
  };
  globalThis.wings = {
    exec: async (program, args, { cwd }) => {
      const reply = fake.reply(args.join(" "), cwd);
      if (reply instanceof Error) throw reply;
      return reply;
    },
    // Wings refuses a message over 64 KB.
    broadcast: async (message) => {
      if (JSON.stringify(message).length > 64_000) throw new Error("A broadcast is limited to 64 KB");
      fake.sent.push(message);
    },
    onBroadcast: (listener) => void (fake.heard = listener),
  };
  return fake;
}
