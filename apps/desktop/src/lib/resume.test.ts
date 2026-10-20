/// <reference types="node" />
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "node:test";

import { canFork, forkLine, isShellCommand, paneCwds, resumeCommand, resumeLine, shellQuote } from "./resume.ts";

const id = "0aa900ec-ed63-4df5-97c1-eecebeb8ef8d";

test("a session saved without flags resumes as before", () => {
  assert.equal(resumeCommand(id), `claude --resume ${id}\r`);
  assert.equal(resumeCommand(id, []), `claude --resume ${id}\r`);
});

test("plain flags are typed as they are", () => {
  assert.equal(resumeCommand(id, ["--model", "opus", "--permission-mode=plan"]), `claude --resume ${id} --model opus --permission-mode=plan\r`);
});

test("quotes spaces, quotes and shell syntax", () => {
  assert.equal(shellQuote("fix the bug"), "'fix the bug'");
  assert.equal(shellQuote("don't"), `'don'\\''t'`);
  assert.equal(shellQuote(""), "''");
  assert.equal(shellQuote("=opus"), "'=opus'");
  assert.equal(shellQuote("$(rm -rf ~)"), "'$(rm -rf ~)'");
});

test("the shell reads back exactly the arguments", () => {
  const args = ["--append-system-prompt", `say "hi", don't ask`, "$HOME `id` !! ~ *", "=opus", "", "ünïcode \\ back\\slash"];
  const line = resumeCommand(id, args).slice(0, -1).replace(/^claude /, "");
  for (const shell of ["/bin/sh", "/bin/bash", "/bin/zsh"]) {
    const out = execFileSync(shell, ["-c", `for a in ${line}; do printf '%s\\0' "$a"; done`]).toString();
    assert.deepEqual(out.split("\0").slice(0, -1), ["--resume", id, ...args], shell);
  }
});

test("a pane is at a prompt only when a shell is its foreground program", () => {
  for (const command of ["zsh", "bash", "fish"]) assert.equal(isShellCommand(command), true, command);
  for (const command of ["claude", "vim", "node", "", undefined]) assert.equal(isShellCommand(command), false, String(command));
});

test("flags holding control characters are not typed", () => {
  assert.equal(resumeCommand(id, ["--append-system-prompt", "line one\nline two"]), `claude --resume ${id}\r`);
  assert.equal(resumeCommand(id, ["--name", "a\x03b"]), `claude --resume ${id}\r`);
});

test("paneCwds keeps each pane's live folder, and the saved one for a pane whose shell hasn't started yet", () => {
  const panes = {
    ui: { paneId: "p1", savedCwd: "/repo" },
    sdk: { paneId: null, savedCwd: "/repo/packages/plugin-sdk" },
    fresh: { paneId: "p3" },
  };
  const info = { p1: { cwd: "/repo/packages/plugin-ui" }, p3: { cwd: null } };
  assert.deepEqual(paneCwds(panes, info), { ui: "/repo/packages/plugin-ui", sdk: "/repo/packages/plugin-sdk" });
});

test("each agent resumes with its own CLI, and only Claude Code forks", () => {
  const id = "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11";
  assert.equal(resumeLine("opencode", "ses_abc12345", ["--model", "x"]), "opencode --session ses_abc12345\r");
  assert.equal(forkLine("claude", id), `claude --resume ${id} --fork-session\r`);
  assert.equal(forkLine("opencode", "ses_abc12345"), null);
  assert.deepEqual([canFork("claude"), canFork("opencode")], [true, false]);
});
