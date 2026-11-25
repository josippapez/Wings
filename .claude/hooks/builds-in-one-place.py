"""Whether a subagent's shell command builds outside `scripts/build.sh`: prints "deny" when it does. Reads the command on
stdin. Called by builds-in-one-place.sh; tested by builds-in-one-place.test.py.

A build is a command the shell runs: `cargo build|test|...`, `build-rust.sh`, `package-mac-app.sh` or
`pnpm [--dir x] shot|test|build`, also after `env`, `time`, `timeout 60`, `VAR=value` and the like, and inside
`sh -c '...'`. A mention is not one: a quoted search for "cargo test", a heredoc's text, a file name in a commit message.
A command substitution or process substitution hides what runs, so with one any mention counts.
"""

import re
import shlex
import sys

BUILD = re.compile(
    r"(^|[^\w-])(cargo\s+(build|test|run|check|clippy|bench|nextest)|build-rust\.sh|package-mac-app\.sh|pnpm(\s+--dir\s+\S+)?\s+(shot|test|build))"
)
CARGO = {"build", "test", "run", "check", "clippy", "bench", "nextest"}
PNPM = {"shot", "test", "build"}
SEPARATORS = {"&&", "||", ";", "|", "&", "(", ")", ";;", "|&"}
# Words that run the command after them.
PREFIXES = {"env", "time", "nice", "exec", "command", "xargs", "sudo", "nohup", "caffeinate"}
SHELLS = {"sh", "bash", "zsh"}


def without_heredocs(command):
    """`command` with each heredoc's text left out: it is data, not commands."""
    kept, marker = [], None
    for line in command.split("\n"):
        if marker is not None:
            if line.strip() == marker:
                marker = None
            continue
        kept.append(line)
        found = re.search(r"<<-?\s*['\"]?(\w+)['\"]?", line)
        if found:
            marker = found.group(1)
    return "\n".join(kept)


def queued(word, rest):
    """Whether `word` (and its first argument) is the build queue."""
    path = rest[0] if word in SHELLS and rest else word
    return path.endswith("scripts/build.sh")


def builds(command):
    try:
        lexer = shlex.shlex(without_heredocs(command).replace("\n", " ; "), posix=True, punctuation_chars=True)
        lexer.whitespace_split = True
        tokens = list(lexer)
    except ValueError:
        # Quotes that don't close: read it as it was before.
        return bool(BUILD.search(command))
    start = 0
    while start < len(tokens):
        end = next((i for i in range(start, len(tokens)) if tokens[i] in SEPARATORS), len(tokens))
        words = tokens[start:end]
        start = end + 1
        while words and (words[0] in PREFIXES or re.fullmatch(r"\w+=.*", words[0]) or words[0] in {"timeout", "-n"} or re.fullmatch(r"-?[\d.]+[smh]?", words[0])):
            words = words[1:]
        if not words:
            continue
        word, rest = words[0], words[1:]
        if queued(word, rest):
            continue
        if word in SHELLS and len(rest) >= 2 and rest[0] == "-c":
            if builds(rest[1]):
                return True
            continue
        if word == "cargo" and any(arg in CARGO for arg in rest[:3]):
            return True
        if word.endswith(("build-rust.sh", "package-mac-app.sh")) or (word in SHELLS and rest and rest[0].endswith(("build-rust.sh", "package-mac-app.sh"))):
            return True
        if word == "pnpm":
            args = rest[2:] if rest[:1] == ["--dir"] else rest
            if args and args[0] in PNPM:
                return True
    return False


def verdict(command):
    if re.search(r"\$\(|`|<\(", command):
        # What runs is hidden in a substitution: any build mentioned outside the queue is refused.
        for part in re.split(r"&&|\|\||;|\||&|\n", command):
            if BUILD.search(part) and not re.match(r"^\s*(sh\s+)?(\S*/)?scripts/build\.sh(\s|$)", part):
                return "deny"
        return "allow"
    return "deny" if builds(command) else "allow"


if __name__ == "__main__":
    print(verdict(sys.stdin.read()))
