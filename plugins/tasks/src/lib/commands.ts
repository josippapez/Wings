// The commands that start Claude in a pane.

import { KEY_PATTERN } from "./domain";

/**
 * Pane commands are typed into the user's shell: Wings takes command words of letters, digits and -_./:=@%+, and
 * quotes each `args` value as one word itself, spaces and all. Nothing here is user text: a key and a session id are
 * checked first.
 */
export const PLAIN_WORD = /^[A-Za-z0-9_.\/:=@%+,-]+$/;
export function resumeCommand(sessionId: string): string {
  if (!/^[0-9A-Za-z-]{8,64}$/.test(sessionId)) throw new Error("That isn't a Claude session id.");
  return `claude --resume ${sessionId}`;
}
/** `claude` with its first prompt, as `openPane`'s `command` and `args`. */
export function agentCommand(issueKey: string): { command: string; args: string[] } {
  if (!KEY_PATTERN.test(issueKey)) throw new Error("That isn't an issue key.");
  const prompt = `Continue issue ${issueKey}. Read it with tasks__get, do the work, and log progress with tasks__log and the status with tasks__update as you go.`;
  return { command: "claude", args: [prompt] };
}
