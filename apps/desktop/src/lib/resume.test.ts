/// <reference types="node" />
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "node:test";

import { resumeCommand, shellQuote } from "./resume.ts";

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

test("flags holding control characters are not typed", () => {
  assert.equal(resumeCommand(id, ["--append-system-prompt", "line one\nline two"]), `claude --resume ${id}\r`);
  assert.equal(resumeCommand(id, ["--name", "a\x03b"]), `claude --resume ${id}\r`);
});
