// GitHub reads that one `gh` call answers for many repositories, so a plugin that lists the pull requests of every
// project starts one process, not two per project. The plugin lists `gh graphql` in `permissions.post`: `gh api graphql`
// takes its query in a field, which `exec` refuses.
import { batched } from "./async.js";
import { CliError } from "./cli.js";

const DOTCOM = "github.com";
/** Open PRs read per repository: what `gh pr list --limit 100` read. */
const PULLS_PER_REPO = 100;
/**
 * Repositories per query. GitHub refuses a query that could return over 500,000 nodes, and a repository of 100 PRs
 * with 100 checks each counts about 10,000.
 */
const REPOS_PER_QUERY = 25;

/** The data of a GraphQL answer that came with errors, which `gh api` exits 1 for, or null when it has none. */
function partial(error) {
  if (!(error instanceof CliError) || error.kind !== "failed") return null;
  try {
    const answer = JSON.parse(error.stdout);
    return answer?.data ? answer : null;
  } catch {
    return null;
  }
}

/**
 * The open pull requests of GitHub repositories, read through `gh api graphql`: one call for every repository asked in
 * the same microtask turn, and in it the numbers of the PRs that ask for your review (yours or your team's), which one
 * search answers for every repository. `selection` is what to read of each PullRequest, as GraphQL text. `host` is a
 * GitHub Enterprise Server, `key` what the CLI records a sign-in problem under. `get({ owner, name })` resolves the
 * repository's PRs, newest first, up to 100, and `asked`, the numbers of the ones among them that ask for your review
 * (of the first 100 review requests over every repository). A repository that can't be read rejects its own `get`.
 */
export function githubPulls(gh, { selection, host = DOTCOM, key }) {
  return batched(
    async (repos) => {
      const quote = JSON.stringify;
      const reads = repos.map((r, i) => `r${i}: repository(owner: ${quote(r.owner)}, name: ${quote(r.name)}) { pullRequests(states: OPEN, first: ${PULLS_PER_REPO}, orderBy: { field: CREATED_AT, direction: DESC }) { nodes { ${selection} } } }`);
      const ask = `asked: search(query: "is:pr is:open review-requested:@me", type: ISSUE, first: 100) { nodes { ... on PullRequest { number repository { nameWithOwner } } } }`;
      const call = { key, host: host === DOTCOM ? undefined : host };
      const query = `query { ${reads.join(" ")} ${ask} }`;
      let answer;
      try {
        answer = JSON.parse(await gh.post("graphql", { query }, call));
      } catch (error) {
        answer = partial(error);
        if (!answer) throw error;
      }
      const asked = answer.data.asked?.nodes ?? [];
      return repos.map((r, i) => {
        const found = answer.data[`r${i}`];
        if (!found) return new Error(answer.errors?.find((e) => e.path?.[0] === `r${i}`)?.message ?? `${r.owner}/${r.name} couldn't be read`);
        const name = `${r.owner}/${r.name}`.toLowerCase();
        return { pulls: found.pullRequests.nodes, asked: asked.filter((n) => n?.repository?.nameWithOwner.toLowerCase() === name).map((n) => n.number) };
      });
    },
    { max: REPOS_PER_QUERY, key: (r) => `${r.owner}/${r.name}`.toLowerCase() },
  );
}
