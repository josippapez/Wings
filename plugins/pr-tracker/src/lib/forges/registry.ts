/** The forges by provider, and the calls that find the one a link, a remote or a tab's args belong to. This is the only place that knows which providers there are. */
import { azure } from "./azure";
import { bitbucket } from "./bitbucket";
import { gitea } from "./gitea";
import { github } from "./github";
import { gitlab } from "./gitlab";
import type { Forge } from "./types";
import type { KnownRemote, Link, PrArgs, Provider, Remote } from "../types";

export const forges: Record<Provider, Forge> = { github, azure, gitlab, bitbucket, gitea };

/** The forge a link, or a tab's args, belong to. */
export const forgeOf = (of: { provider: Provider }): Forge => forges[of.provider];

/** Whether a remote names a provider: not a folder with no remote, nor a host that no CLI knows yet. */
export const isKnown = (remote: Remote): remote is KnownRemote => remote.kind !== "none" && remote.kind !== "unknown";

/** The forge of a remote. */
export const forgeFor = (remote: KnownRemote): Forge => forges[remote.kind];

/** The link a tab's args name. */
export const linkOf = (args: PrArgs): Promise<Link> => forgeOf(args).linkOf(args);
