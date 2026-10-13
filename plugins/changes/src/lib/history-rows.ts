import type { CommitRow } from "./types";

/**
 * The History tab's rows once a first page arrives for the list it already shows. The rows after that page that are
 * still in the old list stay, so a poll after a new commit doesn't drop the pages Load more added (and scroll you up).
 * When the page's last commit isn't among the shown rows (history was rewritten), the page replaces them.
 */
export function withFirstPage(shown: CommitRow[], page: CommitRow[]) {
  const last = page.at(-1)?.[0];
  const at = last === undefined ? -1 : shown.findIndex(([hash]) => hash === last);
  return at < 0 ? page : [...page, ...shown.slice(at + 1)];
}
