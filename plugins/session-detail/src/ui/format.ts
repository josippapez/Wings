export const took = (ms: number | null | undefined) => (ms == null ? "" : ms < 1000 ? `${ms} ms` : ms < 60_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms / 60_000)} min`);
export const status: Record<string, string> = { working: "Working", blocked: "Waiting for you", done: "Finished", idle: "Idle" };
