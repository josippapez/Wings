/** What a pull request's provider is called and looks like, for the badge, the block and the PR tab. No I/O, so a page can import it. */
import type { Link, Provider } from "./types";

export const PROVIDERS: Record<Provider, { name: string; tone: Tone; prefix: "#" | "!" }> = {
  github: { name: "GitHub", tone: "merged", prefix: "#" },
  azure: { name: "Azure DevOps", tone: "warning", prefix: "#" },
  // GitLab numbers merge requests `!12`; the others use `#12`.
  gitlab: { name: "GitLab", tone: "info", prefix: "!" },
  bitbucket: { name: "Bitbucket", tone: "danger", prefix: "#" },
  // The only host the manifest's `fetch` reaches is codeberg.org.
  gitea: { name: "Codeberg", tone: "success", prefix: "#" },
};

export const refOf = (link: Pick<Link, "provider" | "number">) => `${PROVIDERS[link.provider].prefix}${link.number}`;
