#!/usr/bin/env python3
"""Writes src/terminal_commands.rs: Claude Code's slash commands that have a form only its terminal runs, read from the
installed Claude Code itself.

Claude Code lists only the commands its headless mode runs (`initialize`'s `commands`): a skill or prompt command,
unless it sets `disableNonInteractive`, and a `local` command with `supportsNonInteractive`. Every other command in
its registry (`local-jsx` panels, and `local` ones without that flag) answers `-p` with "/name isn't available in this
environment." Some names have both forms (`/model` opens a picker in the terminal and takes a name in `-p`), and
whether `-p` has its form on is only known as it runs, so the chat goes by what `initialize` lists and this table is
for the rest. The registry is bundled JavaScript inside the binary, so this finds each command object in it. The
public ones are kept: those Claude Code's commands page lists, as hidden or internal ones aren't for you.

    python3 terminal_commands.py            # write the table from the installed Claude Code and its docs
    python3 terminal_commands.py --check    # exit 1 when the installed Claude Code's set differs from the table's

`--check` reads no network: it compares the names of every command `-p` can't run, documented or not.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
TABLE = os.path.join(HERE, "src", "terminal_commands.rs")
DOCS = "https://code.claude.com/docs/en/commands.md"


def binary():
    """The installed Claude Code: the file `claude` on the PATH leads to, which is the binary of one version."""
    found = shutil.which("claude")
    if not found:
        return None, None
    path = os.path.realpath(found)
    version = subprocess.run([found, "--version"], capture_output=True, text=True, timeout=60).stdout.split()[0]
    return path, version


def objects(text):
    """Each object literal with a `type:"local-jsx"`, `"local"` or `"prompt"` of its own, as text."""
    for m in re.finditer(r'type:"(local-jsx|local|prompt)"', text):
        p = m.start()
        for s in (i for i in range(p - 1, max(0, p - 4000), -1) if text[i] == "{"):
            end = closing(text, s, p)
            if end is not None:
                yield text[s : end + 1]
                break


def closing(text, s, p):
    """Where the `{` at `s` closes, when `p` sits in it directly and not in something nested."""
    depth, i, n = 0, s, len(text)
    while i < n and i < s + 20000:
        c = text[i]
        if c in "\"'`":
            i = past_string(text, i)
            continue
        if c in "{[(":
            depth += 1
        elif c in "}])":
            depth -= 1
            if depth == 0:
                return i if c == "}" and s < p < i else None
        if i == p and depth != 1:
            return None
        i += 1
    return None


def past_string(text, i):
    quote, i = text[i], i + 1
    while i < len(text):
        if text[i] == "\\":
            i += 2
            continue
        if quote == "`" and text.startswith("${", i):
            depth, i = 1, i + 2
            while i < len(text) and depth:
                depth += {"{": 1, "}": -1}.get(text[i], 0)
                i += 1
            continue
        if text[i] == quote:
            return i + 1
        i += 1
    return i


def fields(obj):
    """The object's own keys, each with the text of its value (a getter's body for `get key(){...}`)."""
    body, parts, depth, start, i = obj[1:-1], [], 0, 0, 0
    while i < len(body):
        c = body[i]
        if c in "\"'`":
            i = past_string(body, i)
            continue
        if c in "{[(":
            depth += 1
        elif c in "}])":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(body[start:i])
            start = i + 1
        i += 1
    parts.append(body[start:])
    found = {}
    for part in parts:
        m = re.match(r"\s*(?:get\s+)?([A-Za-z_$][\w$]*)\s*(?:\(\)\s*\{|:)(.*)$", part, re.S)
        if m:
            found.setdefault(m.group(1), m.group(2).strip())
    return found


def string(value):
    """A value that is a string literal, or a getter that returns one first."""
    m = re.match(r'"((?:[^"\\]|\\.)*)"', value or "") or re.match(r'return\s*"((?:[^"\\]|\\.)*)"', value or "")
    return json.loads(f'"{m.group(1)}"') if m else None


def strings(value):
    m = re.match(r"\[(.*)\]$", value or "", re.S)
    return [json.loads(s) for s in re.findall(r'"(?:[^"\\]|\\.)*"', m.group(1))] if m else []


def registry(path):
    """Every command in the binary by name with the forms of it that `-p` can't run."""
    text = open(path, "rb").read().decode("latin-1")
    commands = {}
    for obj in objects(text):
        f = fields(obj)
        name, kind = string(f.get("name")), string(f.get("type"))
        if not name or not re.fullmatch(r"[a-z][a-z0-9-]*", name):
            continue
        headless = (kind == "prompt" and f.get("disableNonInteractive") != "!0") or (kind == "local" and f.get("supportsNonInteractive") == "!0")
        forms = commands.setdefault(name, [])
        if not headless:
            forms.append({
                "aliases": strings(f.get("aliases")),
                "hint": string(f.get("argumentHint")) or "",
                "description": string(f.get("description")) or "",
                # Off for everyone, or hidden from every menu: not a command anyone can use.
                "gone": f.get("isEnabled") == "()=>!1" or f.get("isHidden") == "!0",
            })
    return {name: forms for name, forms in commands.items() if forms}


def unrunnable(found):
    return sorted(name for name, forms in found.items() if not all(form["gone"] for form in forms))


def documented():
    """The commands page's table: each command's name and its first sentence, without links or code marks."""
    request = urllib.request.Request(DOCS, headers={"User-Agent": "wings-claude-chat-table"})
    page = urllib.request.urlopen(request, timeout=60).read().decode()
    rows = {}
    for line in page.splitlines():
        m = re.match(r"^\| `/([a-z0-9:-]+)[^`]*` \| (.*?)\s*\|\s*$", line)
        if m:
            said = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", m.group(2)).replace("`", "").replace("**", "")
            rows[m.group(1)] = re.split(r"(?<=[A-Za-z0-9)])\. ", said)[0].rstrip(".")
    return rows


def rust(text):
    return json.dumps(text, ensure_ascii=False)


def write(version, found, docs):
    names = unrunnable(found)
    rows = []
    for name in names:
        if name not in docs or docs[name].startswith("Removed"):
            continue
        forms = [form for form in found[name] if not form["gone"]]
        aliases = sorted({alias for form in forms for alias in form["aliases"] if alias != name})
        hint = next((form["hint"] for form in forms if form["hint"]), "")
        description = next((form["description"] for form in forms if form["description"]), "") or docs[name]
        rows.append(f"    Terminal {{ name: {rust(name)}, aliases: &[{', '.join(rust(a) for a in aliases)}], hint: {rust(hint)}, description: {rust(description)} }},")
    listed = "\n".join(f"    {rust(name)}," for name in names)
    out = f"""//! Generated by `terminal_commands.py` from Claude Code {version} and its commands page: run it again rather than edit
//! this. Claude Code's slash commands that have a form only its terminal runs: unless `initialize` lists the name too (as
//! it does `/model`, which takes a name in `-p`), `claude -p` answers it with "/name isn't available in this environment."

#[derive(Debug, PartialEq)]
pub struct Terminal {{
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub hint: &'static str,
    /// Claude Code's own words for it, or the docs' where it has none of its own.
    pub description: &'static str,
}}

/// The Claude Code these were read from.
pub const VERSION: &str = {rust(version)};

/// The ones its commands page lists, in name order.
pub const COMMANDS: &[Terminal] = &[
{chr(10).join(rows)}
];

/// Every command of that Claude Code with such a form, listed on the page or not, for the test that compares them with the
/// installed Claude Code.
pub const UNRUNNABLE: &[&str] = &[
{listed}
];
"""
    with open(TABLE, "w") as f:
        f.write(out)
    missing = sorted(set(docs) - set(found))
    print(f"Wrote {len(rows)} commands from Claude Code {version} to {TABLE}", file=sys.stderr)
    print(f"On the commands page with no terminal form in the registry: {', '.join(missing)}", file=sys.stderr)


def check(found):
    table = open(TABLE).read()
    kept = re.findall(r'^    "([a-z0-9-]+)",$', table.split("pub const UNRUNNABLE")[1], re.M)
    now = unrunnable(found)
    if kept == now:
        print("The table matches the installed Claude Code.")
        return 0
    print(f"Claude Code changed its terminal commands: new {sorted(set(now) - set(kept))}, gone {sorted(set(kept) - set(now))}.")
    print(f"Run `python3 {os.path.relpath(__file__, os.path.join(HERE, '..', '..', '..'))}` from the repo to write the table again.")
    return 1


def main():
    path, version = binary()
    if path is None:
        print("skip: no claude on the PATH")
        return 0
    found = registry(path)
    if "--check" in sys.argv:
        return check(found)
    write(version, found, documented())
    return 0


if __name__ == "__main__":
    sys.exit(main())
