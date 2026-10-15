/** Writing through a CLI: a reply or an approval, which `wings.post` sends as you to the paths the manifest lists. */

/** What the CLI printed for a POST, or its error. */
export async function post(program: "gh" | "glab" | "az", url: string, fields: Record<string, string | number>, options?: { host?: string }): Promise<unknown> {
  const out = await wings.post(program, url, fields, options);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || `${program} failed`);
  return JSON.parse(out.stdout) as unknown;
}
