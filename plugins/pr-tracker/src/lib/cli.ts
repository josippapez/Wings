/** Running git and the providers' CLIs. */
import { CLIS, problems } from "./state";
import type { Cli } from "./types";

/** How `wings.exec` fails for a program that isn't installed. */
const NOT_INSTALLED = /is not installed or not on PATH/;

/** Runs a provider's CLI, and remembers under `key` when it isn't installed, so the pane offers to install it. */
export async function cli(key: string, program: Cli, args: string[], options?: WingsExecOptions) {
  try {
    return await wings.exec(program, args, options);
  } catch (error) {
    const message = String((error as Error).message ?? error);
    if (NOT_INSTALLED.test(message)) problems.set(key, { missing: program, message: `PR tracker needs the ${CLIS[program]} (${program}) to find this branch's pull request.` });
    throw error;
  }
}

export async function git(cwd: string, args: string[]) {
  const out = await wings.exec("git", args, { cwd });
  // ssh prints warnings, like OpenSSH's post-quantum notice, as "** " lines before the real error.
  const error = out.stderr.split("\n").filter((l) => !l.startsWith("** ")).join("\n").trim();
  if (out.code !== 0) throw new Error(error || `git ${args[0]} failed`);
  return out.stdout.trim();
}

/** The CLI's error, or the response it printed. */
export async function post(program: Cli, url: string, fields: Record<string, string | number>, options?: { host?: string }) {
  const out = await wings.post(program, url, fields, options);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || `${program} failed`);
  return JSON.parse(out.stdout) as unknown;
}
