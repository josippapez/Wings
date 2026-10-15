export const took = (ms: number | null | undefined) => (ms == null ? "" : ms < 1000 ? `${ms} ms` : ms < 60_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms / 60_000)} min`);
export const clock = (at: number) => new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
export const status: Record<string, string> = { working: "Working", blocked: "Waiting for you", done: "Finished", idle: "Idle" };
export const basename = (path: string) => path.split("/").filter(Boolean).at(-1) ?? path;
