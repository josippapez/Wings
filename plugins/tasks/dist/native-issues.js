// Built by node plugins/tasks/build-native.mjs. Edit worker/issues.ts, not this file.
"use strict";
(() => {
  // worker/domain.ts
  var LIMITS = { issues: 300, title: 200, description: 2e4, note: 2e4, branch: 200, pr: 500, log: 300, key: 20, commit: 40, query: 200, files: 100, filePath: 300, fileWhat: 300 };
  var FORMAT_VERSION = 3;
  var STATUSES = ["todo", "in_progress", "in_review", "done", "canceled"];
  var TYPES = ["task", "bug", "feature", "epic"];
  var isType = (v) => typeof v === "string" && TYPES.includes(v);
  var isStatus = (v) => typeof v === "string" && STATUSES.includes(v);
  function prefixFor(name) {
    return (name.match(/[A-Za-z]/g) ?? []).slice(0, 3).join("").toUpperCase() || "ISS";
  }
  var keyNumber = (key) => Number(key.slice(key.lastIndexOf("-") + 1));
  var KEY_PATTERN = /^[A-Z]{1,3}-\d{1,9}$/;

  // worker/storage.ts
  var emptyStore = (projectName) => ({ v: FORMAT_VERSION, prefix: prefixFor(projectName), next: 1, issues: [] });
  function slotOf(projectId) {
    let h1 = 3735928559;
    let h2 = 1103547991;
    for (let i = 0; i < projectId.length; i++) {
      const c = projectId.charCodeAt(i);
      h1 = Math.imul(h1 ^ c, 2654435761);
      h2 = Math.imul(h2 ^ c, 1597334677);
    }
    h1 = Math.imul(h1 ^ h1 >>> 16, 2246822507) ^ Math.imul(h2 ^ h2 >>> 13, 3266489909);
    h2 = Math.imul(h2 ^ h2 >>> 16, 2246822507) ^ Math.imul(h1 ^ h1 >>> 13, 3266489909);
    return (h2 >>> 0).toString(16).padStart(8, "0") + (h1 >>> 0).toString(16).padStart(8, "0");
  }
  var projectKey = (projectId) => `issues/${slotOf(projectId)}`;
  var oldKey = (projectId) => `issues:${projectId}`;
  var legacyKey = (projectId) => `tasks:${projectId}`;
  var isShape = (v, version) => !!v && typeof v === "object" && v.v === version && Array.isArray(v.issues) && typeof v.prefix === "string" && typeof v.next === "number";
  var isStore = (v) => isShape(v, FORMAT_VERSION);
  var isProject = (v) => !!v && typeof v === "object" && v.v === FORMAT_VERSION && typeof v.path === "string" && typeof v.prefix === "string" && typeof v.next === "number";
  function migrateV2(value) {
    if (!isShape(value, 2)) return null;
    const old = value;
    return { v: FORMAT_VERSION, prefix: old.prefix, next: old.next, issues: old.issues.map((i) => ({ ...i, type: "task" })) };
  }
  function migrateTasks(value, projectName) {
    const store = emptyStore(projectName);
    if (!Array.isArray(value)) return store;
    const old = value.filter((t) => t && typeof t.title === "string" && typeof t.status === "string").map((t, i) => ({ t, i, at: typeof t.createdAt === "number" ? t.createdAt : 0 })).sort((a, b) => a.at - b.at || a.i - b.i);
    for (const { t, at } of old) {
      const status = t.status === "doing" ? "in_progress" : t.status === "done" ? "done" : "todo";
      const issue = {
        key: `${store.prefix}-${store.next++}`,
        title: t.title.slice(0, LIMITS.title),
        status,
        type: "task",
        sessions: [],
        log: [{ at, by: "you", kind: "created", text: "Imported from Tasks" }],
        createdAt: at,
        updatedAt: typeof t.updatedAt === "number" ? t.updatedAt : at
      };
      if (typeof t.notes === "string" && t.notes.trim()) issue.description = t.notes.slice(0, LIMITS.description);
      if (typeof t.links?.branch === "string" && t.links.branch) issue.links = { branch: t.links.branch.slice(0, LIMITS.branch) };
      store.issues.push(issue);
    }
    return store;
  }
  async function readStore(projectId, projectName) {
    const entries = await wings.storage.entries(projectKey(projectId));
    const project = entries.find(([k]) => k === projectKey(projectId))?.[1];
    if (!isProject(project)) return readOld(projectId, projectName);
    if (project.path !== projectId) throw new Error(`Issues keeps ${project.path} where ${projectId} would go. Rename one of the two folders.`);
    const issues = entries.filter(([k]) => k.startsWith(`${projectKey(projectId)}/`)).map(([, v]) => v);
    return { v: FORMAT_VERSION, prefix: project.prefix, next: project.next, issues: issues.sort((a, b) => keyNumber(a.key) - keyNumber(b.key)) };
  }
  async function readOld(projectId, projectName) {
    const value = await wings.storage.get(oldKey(projectId));
    if (isStore(value)) return value;
    const v2 = migrateV2(value);
    if (v2) return v2;
    return migrateTasks(await wings.storage.get(legacyKey(projectId)), projectName);
  }

  // worker/issues.ts
  var MAX_ROWS = 2e3;
  var MAX_ERROR = 500;
  var record = (value) => !!value && typeof value === "object" && !Array.isArray(value);
  async function checkSource(project) {
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
      if (!record(old) || old.v !== FORMAT_VERSION && old.v !== 2 || typeof old.prefix !== "string" || typeof old.next !== "number" || !Number.isFinite(old.next) || !Array.isArray(old.issues)) {
        throw new Error("The stored Issues list is malformed or uses an unsupported format.");
      }
      return;
    }
    const tasks = await wings.storage.get(`tasks:${project.id}`);
    if (tasks !== null && (!Array.isArray(tasks) || migrateTasks(tasks, project.name).issues.length !== tasks.length)) {
      throw new Error("The stored Tasks list is malformed.");
    }
  }
  function rowOf(value) {
    if (!record(value) || typeof value.key !== "string" || value.key.length > LIMITS.key || !KEY_PATTERN.test(value.key) || typeof value.title !== "string" || !value.title.trim() || value.title.length > LIMITS.title || !isStatus(value.status) || !isType(value.type)) {
      throw new Error("A stored issue has an invalid key, title, status or type.");
    }
    return { id: value.key, title: value.title, status: value.status, type: value.type };
  }
  var issuesScreen = async (project, action) => {
    const screen = { version: 1, title: "Issues", state: "empty", rows: [], actions: [{ id: "refresh", label: "Refresh" }] };
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
})();
