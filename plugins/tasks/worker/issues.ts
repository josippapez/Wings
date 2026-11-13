import type { NativeEntrypoint, NativeProject, NativeRow, NativeScreen } from "../../../packages/plugin-sdk/native";
import { FORMAT_VERSION, isStatus, isType, KEY_PATTERN, LIMITS } from "./domain";
import { migrateTasks, readStore, slotOf } from "./storage";

const MAX_ROWS = 2_000;
const MAX_ERROR = 500;
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);

/** readStore deliberately falls back on unknown formats. Native reads must report damaged data instead of an empty list. */
async function checkSource(project: NativeProject): Promise<void> {
  const key = `issues/${slotOf(project.id)}`;
  const entries = await wings.storage.entries(key);
  const entry = entries.find(([k]) => k === key);
  if (entry) {
    const value = entry[1];
    if (!record(value) || value.v !== FORMAT_VERSION || typeof value.path !== "string" || typeof value.prefix !== "string" || typeof value.next !== "number" || !Number.isFinite(value.next)) {
      throw new Error("The stored Issues project is malformed.");
    }
    return;
  }
  const old = await wings.storage.get(`issues:${project.id}`);
  if (old !== null) {
    if (!record(old) || (old.v !== FORMAT_VERSION && old.v !== 2) || typeof old.prefix !== "string" || typeof old.next !== "number" || !Number.isFinite(old.next) || !Array.isArray(old.issues)) {
      throw new Error("The stored Issues list is malformed or uses an unsupported format.");
    }
    return;
  }
  const tasks = await wings.storage.get(`tasks:${project.id}`);
  if (tasks !== null && (!Array.isArray(tasks) || migrateTasks(tasks, project.name).issues.length !== tasks.length)) {
    throw new Error("The stored Tasks list is malformed.");
  }
}

function rowOf(value: unknown): NativeRow {
  if (!record(value) || typeof value.key !== "string" || value.key.length > LIMITS.key || !KEY_PATTERN.test(value.key) || typeof value.title !== "string" || !value.title.trim() || value.title.length > LIMITS.title || !isStatus(value.status) || !isType(value.type)) {
    throw new Error("A stored issue has an invalid key, title, status or type.");
  }
  return { id: value.key, title: value.title, status: value.status, type: value.type };
}

export const issuesScreen: NativeEntrypoint = async (project, action) => {
  const screen: NativeScreen = { version: 1, title: "Issues", state: "empty", rows: [], actions: [{ id: "refresh", label: "Refresh" }] };
  try {
    if (!record(project) || typeof project.id !== "string" || !project.id || typeof project.name !== "string") throw new Error("Issues needs a project id and name.");
    if (action !== null && action !== "refresh") throw new Error("Issues only supports Refresh.");
    await checkSource(project);
    const store = await readStore(project.id, project.name);
    if (store.issues.length > MAX_ROWS) throw new Error(`Issues can display at most ${MAX_ROWS} rows. This project has ${store.issues.length}.`);
    const rows = store.issues.map(rowOf);
    if (new Set(rows.map((row) => row.id)).size !== rows.length) throw new Error("The stored Issues list has duplicate keys.");
    return { ...screen, state: rows.length ? "ready" : "empty", rows };
  } catch (error) {
    return { ...screen, state: "error", error: (error instanceof Error ? error.message : String(error)).slice(0, MAX_ERROR) || "Couldn't read Issues." };
  }
};

globalThis.wingsNative = issuesScreen;
