import type { HighlighterCore } from "shiki/core"

/** A run of code in one color: the text and, when the theme has one, its `#rrggbb`. */
export type HighlightedToken = [text: string, color?: string]

/**
 * How long one line may take to tokenize. The first lines after a grammar loads are slow while the engine compiles its
 * patterns (about 0.9 s for the first tsx lines in WebKit), and Shiki's default of 500 ms gives up mid-line there and
 * leaves the rest of the code uncolored, for good once the page has cached that answer. The size of a block and the
 * length of a line are already bounded, so this only has to outlast a cold start.
 */
export const TOKENIZE_TIME_LIMIT = 10_000

/** The code as tokens per line in the dark theme, which is the one plugin pages are drawn in. */
export function tokensOf(shiki: HighlighterCore, code: string, lang: string): HighlightedToken[][] {
  return shiki
    .codeToTokens(code, { lang, theme: "github-dark", tokenizeMaxLineLength: 4000, tokenizeTimeLimit: TOKENIZE_TIME_LIMIT })
    .tokens.map((line) => line.map((token) => (token.color ? [token.content, token.color] : [token.content])))
}
