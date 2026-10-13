/** Calling a token-authenticated forge (Bitbucket Cloud, Gitea/Forgejo) with `wings.fetch`, which sends the token from the keychain as a Bearer header. */
import { problems } from "./state";

export interface Forge {
  /** Where its login problem is kept, like `bitbucket`. */
  key: string;
  /** The name of the secret the token is saved under in Settings. */
  secret: string;
  name: string;
  /** What a 403 most likely means, since a token without the scope is refused with it. */
  scopeHint: string;
  /** The same for a write (a reply, an approval), which needs its own scopes. */
  writeHint: string;
}

/** Why a call needs the user's attention, which the badge and the block turn into an Add token button. */
function needsToken(forge: Forge, message: string): never {
  problems.set(forge.key, { login: true, token: true, message });
  throw new Error(message);
}

/** The body of a successful GET. A missing or refused token is recorded as a login problem; any other failure just throws. */
export async function forgeGet(forge: Forge, url: string, accept = "application/json"): Promise<string> {
  if (!(await wings.secrets.has(forge.secret))) needsToken(forge, `Add a ${forge.name} token in Settings to see this branch's pull request.`);
  const res = await wings.fetch(url, { headers: { Accept: accept }, bearer: forge.secret });
  if (res.status === 401) needsToken(forge, `${forge.name} refused the token. Add a new one in Settings.`);
  if (res.status === 403) throw new Error(`${forge.name} refused the request (403). ${forge.scopeHint}`);
  if (res.status === 404) throw new Error(`${forge.name} found no such repository, or the token can't see it.`);
  if (res.status < 200 || res.status >= 300) throw new Error(`${forge.name} answered ${res.status}.`);
  problems.delete(forge.key);
  return res.body;
}

/**
 * POSTs a JSON body (none for an action like approve) and returns the answer's body. Only a URL that the manifest's
 * `POST` entries name gets through, with the body they pin.
 */
export async function forgePost(forge: Forge, url: string, body?: object): Promise<string> {
  if (!(await wings.secrets.has(forge.secret))) needsToken(forge, `Add a ${forge.name} token in Settings to reply or review.`);
  const res = await wings.fetch(url, {
    method: "POST",
    headers: { Accept: "application/json", ...(body ? { "Content-Type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
    bearer: forge.secret,
  });
  if (res.status === 401) needsToken(forge, `${forge.name} refused the token. Add a new one in Settings.`);
  if (res.status === 403) throw new Error(`${forge.name} refused it (403). ${forge.writeHint}`);
  if (res.status === 404) throw new Error(`${forge.name} found no such pull request, or the token can't see it.`);
  if (res.status < 200 || res.status >= 300) throw new Error(`${forge.name} answered ${res.status}: ${message(res.body)}`);
  return res.body;
}

/** The reason a forge gave in an error body, for the user to read. */
function message(body: string): string {
  try {
    const m = JSON.parse(body) as { message?: string; error?: { message?: string } };
    return String(m.error?.message ?? m.message ?? body).slice(0, 200);
  } catch {
    return body.slice(0, 200);
  }
}

export async function forgeJson<T>(forge: Forge, url: string): Promise<T> {
  return JSON.parse(await forgeGet(forge, url)) as T;
}

/** The signed-in user's id per secret, so mine-or-review is decided without asking again. */
const users = new Map<string, string>();

export async function forgeUser<T>(forge: Forge, url: string, pick: (user: T) => string | undefined): Promise<string> {
  let id = users.get(forge.secret);
  if (!id) {
    id = pick(await forgeJson<T>(forge, url));
    if (!id) throw new Error(`${forge.name} didn't say who the token belongs to.`);
    users.set(forge.secret, id);
  }
  return id;
}

/** Forgets the users, so a token that was replaced is read again. */
export function forgetUsers() {
  users.clear();
}
