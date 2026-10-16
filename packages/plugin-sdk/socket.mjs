// Talks to the running Wings over its socket, like `wings plugin` does (cli.rs). One request per connection:
// a line of JSON with `op` and `id`, and the reply is the line with the same id. Wings also sends events on the
// connection (tools_changed, and blank lines to check the client is there), which are skipped.
import { createConnection } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";

/** `identifier` in tauri.conf.json. Tauri names the app's data folder after it. */
export const IDENTIFIER = "dev.wings.app";

/** Why a request failed: `notRunning`, `unknownOp` (an older Wings), or `failed` with Wings' own text. */
export class WingsError extends Error {
  constructor(kind, message) {
    super(message);
    this.kind = kind;
  }
}

/** The socket Wings listens on: `WINGS_SOCKET`, or mcp.sock in the app data folder like `dirs::data_dir` finds it. */
export function socketPath(env = process.env, platform = process.platform, home = homedir()) {
  if (env.WINGS_SOCKET) return env.WINGS_SOCKET;
  if (platform === "win32") throw new WingsError("failed", "Plugins aren't available on Windows yet");
  const data = platform === "darwin" ? join(home, "Library", "Application Support") : env.XDG_DATA_HOME || join(home, ".local", "share");
  return join(data, IDENTIFIER, "mcp.sock");
}

/** Sends one request and resolves with Wings' `result`. */
export function ask(op, request = {}, { path = socketPath() } = {}) {
  return new Promise((resolve, reject) => {
    const socket = createConnection(path);
    let buffer = "";
    let done = false;
    const finish = (error, result) => {
      if (done) return;
      done = true;
      socket.destroy();
      error ? reject(error) : resolve(result);
    };
    socket.on("connect", () => socket.write(`${JSON.stringify({ ...request, op, id: 1 })}\n`));
    socket.on("data", (chunk) => {
      buffer += chunk;
      const lines = buffer.split("\n");
      buffer = lines.pop();
      for (const line of lines) {
        let message;
        try {
          message = JSON.parse(line);
        } catch {
          continue;
        }
        if (message?.id !== 1) continue;
        if (typeof message.error === "string") {
          finish(new WingsError(message.error === "unknown op" ? "unknownOp" : "failed", message.error));
        } else {
          finish(null, message.result ?? null);
        }
        return;
      }
    });
    socket.on("error", (e) => finish(e.code === "ENOENT" || e.code === "ECONNREFUSED" ? new WingsError("notRunning", "Wings isn't running") : e));
    socket.on("close", () => finish(new WingsError("failed", "Wings closed the connection")));
  });
}

/** Has Wings read this plugin folder again and restart its plugin. Resolves with the plugin as Wings lists it. */
export const link = (folder, options) => ask("link", { folder }, options);
