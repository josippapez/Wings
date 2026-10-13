// Whose a listed pull request is: yours, or waiting for your review, from what each provider's list says. No `wings`,
// no I/O, so node tests drive it.

export interface Who {
  mine: boolean;
  reviewing: boolean;
}

const same = (a: string | undefined | null, b: string) => Boolean(a) && a!.toLowerCase() === b.toLowerCase();

/**
 * `me` is the login `gh api user` gives. `reviewRequests` also lists teams, which have a slug and no login, so
 * `requested` holds the numbers `gh pr list --search review-requested:@me` found: that search includes your teams.
 */
export function ghWho(pr: { number?: number; author?: { login?: string }; reviewRequests?: { login?: string }[] }, me: string, requested: ReadonlySet<number> = new Set()): Who {
  return { mine: same(pr.author?.login, me), reviewing: requested.has(pr.number!) || (pr.reviewRequests ?? []).some((r) => same(r.login, me)) };
}

/** `me` is the username `glab api user` gives. */
export function glWho(mr: { author?: { username?: string }; reviewers?: { username?: string }[] }, me: string): Who {
  return { mine: same(mr.author?.username, me), reviewing: (mr.reviewers ?? []).some((r) => same(r.username, me)) };
}

/** `me` is what `az account show` calls the user, which Azure DevOps knows as `uniqueName`. Any reviewer counts, voted or not, as the sidebar's `--reviewer` query did. */
export function azWho(pr: { createdBy?: { uniqueName?: string }; reviewers?: { uniqueName?: string }[] }, me: string): Who {
  return { mine: same(pr.createdBy?.uniqueName, me), reviewing: (pr.reviewers ?? []).some((r) => same(r.uniqueName, me)) };
}
