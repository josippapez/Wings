// Syntax colors for CodeBlock. Wings does the tokenizing (`wings.highlight`, which loads a grammar the first time its
// language is used), so no highlighter ships in a page. The page shows plain text first and colors in when tokens arrive.
import { useEffect, useState } from "react";

/** File extensions, and the names of files with none, by the language Wings highlights them as. */
const LANGUAGES = {
  bash: "bash", c: "c", cc: "cpp", cjs: "javascript", cpp: "cpp", cs: "csharp", css: "css", cxx: "cpp", go: "go", h: "c", hpp: "cpp",
  htm: "html", html: "html", java: "java", js: "javascript", json: "json", jsonc: "json", jsx: "jsx", kt: "kotlin", md: "markdown",
  mjs: "javascript", mts: "typescript", php: "php", py: "python", rb: "ruby", rs: "rust", scss: "scss", sh: "bash", sql: "sql",
  svg: "xml", swift: "swift", toml: "toml", ts: "typescript", tsx: "tsx", xml: "xml", yaml: "yaml", yml: "yaml", zsh: "bash",
};

/** The language to highlight a file as, from its path, or undefined for one Wings has no grammar for. */
export function languageOf(path) {
  const name = String(path ?? "").split(/[\\/]/).pop().toLowerCase();
  const dot = name.lastIndexOf(".");
  return dot < 0 ? undefined : LANGUAGES[name.slice(dot + 1)];
}

/** More than this and a block stays plain text: tokenizing it would take longer than reading it. */
export const MAX_HIGHLIGHT = 50_000;

/** Token promises by language and code, so the same code is asked for once however many blocks show it. */
const asked = new Map();

function tokensOf(lang, code) {
  const key = `${lang}\0${code}`;
  let promise = asked.get(key);
  if (!promise) {
    // A failed or unknown language leaves the block plain, so both are null.
    promise = Promise.resolve()
      .then(() => wings.highlight(code, lang))
      .catch(() => null);
    if (asked.size >= 200) asked.delete(asked.keys().next().value);
    asked.set(key, promise);
  }
  return promise;
}

/**
 * The code as lines of `[text, color?]` tokens, or null while they load, for a language Wings doesn't know, for code
 * over `MAX_HIGHLIGHT`, and when Wings is too old to have `wings.highlight`. `lines` is how many lines to expect: a
 * result of another count is dropped.
 */
export function useHighlight(code, lang, lines) {
  const [done, setDone] = useState(null);
  useEffect(() => {
    if (!lang || !code.trim() || code.length > MAX_HIGHLIGHT || typeof wings?.highlight !== "function") return undefined;
    let live = true;
    void tokensOf(lang, code).then((tokens) => {
      if (live) setDone({ lang, code, tokens });
    });
    return () => {
      live = false;
    };
  }, [code, lang]);
  const tokens = done && done.lang === lang && done.code === code ? done.tokens : null;
  return tokens && tokens.length === lines ? tokens : null;
}
