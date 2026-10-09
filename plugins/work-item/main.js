// Work item: shows the Azure Boards work item named in each pane's branch, like `feature/1234-login`,
// as a pane badge with its state, sprint and parent, and moves it to its next state.

const BRANCH_POLL_MS = 15_000;
const ITEM_POLL_MS = 60_000;
const SIGN_IN_TIMEOUT_MS = 300_000;
const NOT_FOUND = /TF401232/;

let panes = [];
/** cwd → { kind: "azure", org } | { kind: "none" } */
const remotes = new Map();
/** paneId → { org, id, key } */
const refs = new Map();
/** `${org} ${id}` → normalized work item, { missing: true } or { error } */
const items = new Map();
const inFlight = new Set();
/** Items refreshed because you clicked something; background polls don't show a spinner. */
const manual = new Set();
/** `${org} ${id}` → Promise of the parent's title, fetched once. */
const parents = new Map();
/** `${org} ${project} ${type}` → Promise of the type's states, in workflow order. */
const typeStates = new Map();
/** Azure org → { login, message }, when `az` can't be used there. */
const problems = new Map();
let signingIn = false;

wings.onPanes((list) => {
  panes = list;
  void refreshRefs();
});
setInterval(refreshRefs, BRANCH_POLL_MS);
setInterval(() => [...new Set([...refs.values()].map((r) => r.key))].forEach((key) => refreshItem(key)), ITEM_POLL_MS);

// ---------- finding the work item for a pane ----------

async function git(cwd, args) {
  const out = await wings.exec("git", args, { cwd });
  if (out.code !== 0) throw new Error(out.stderr.trim() || `git ${args[0]} failed`);
  return out.stdout.trim();
}

/** Azure remotes: ssh v3, https dev.azure.com, and the older {org}.visualstudio.com form. */
function parseRemote(url) {
  let m = url.match(/ssh\.dev\.azure\.com:v3\/([^/]+)\/([^/]+)\/([^/]+?)(?:\.git)?$/);
  if (!m) m = url.match(/dev\.azure\.com\/([^/]+)\/([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://dev.azure.com/${m[1]}` };
  m = url.match(/([^/.@]+)\.visualstudio\.com\/(?:DefaultCollection\/)?([^/]+)\/_git\/([^/]+?)(?:\.git)?$/);
  if (m) return { kind: "azure", org: `https://${m[1]}.visualstudio.com` };
  return { kind: "none" };
}

async function remoteFor(cwd) {
  if (!remotes.has(cwd)) {
    try {
      remotes.set(cwd, parseRemote(await git(cwd, ["remote", "get-url", "origin"])));
    } catch {
      remotes.set(cwd, { kind: "none" });
    }
  }
  return remotes.get(cwd);
}

/** The first run of 3+ digits that is a whole path segment or followed by `-`: `feature/1234-login` → 1234. */
function workItemId(branch) {
  return branch.match(/(?:^|[/_-])(\d{3,})(?=-|\/|$)/)?.[1] ?? null;
}

async function refreshRefs() {
  await Promise.all(
    panes.map(async (pane) => {
      const remote = await remoteFor(pane.cwd);
      const branch = remote.kind === "azure" ? await git(pane.cwd, ["branch", "--show-current"]).catch(() => "") : "";
      const id = workItemId(branch);
      if (!id) return void refs.delete(pane.paneId);
      const key = `${remote.org} ${id}`;
      refs.set(pane.paneId, { org: remote.org, id, key });
      if (!items.has(key)) void refreshItem(key);
    }),
  );
  render();
}

// ---------- the work item ----------

async function az(org, args) {
  const out = await wings.exec("az", [...args, "--organization", org, "-o", "json"]);
  if (out.code === 0) {
    problems.delete(org);
    return JSON.parse(out.stdout);
  }
  const message = out.stderr.trim().split("\n").at(-1) || "az failed";
  if (!NOT_FOUND.test(message)) {
    const login = /login/i.test(out.stderr);
    problems.set(org, { login, message: login ? "Sign in to Azure DevOps to see this branch's work item." : message });
  }
  throw new Error(message);
}

const show = (org, id) => az(org, ["boards", "work-item", "show", "--id", String(id)]);

function statesOf(org, project, type) {
  const key = `${org} ${project} ${type}`;
  if (!typeStates.has(key)) {
    const load = az(org, [
      "devops", "invoke", "--area", "wit", "--resource", "workItemTypeStates",
      "--route-parameters", `project=${project}`, `type=${type}`, "--api-version", "7.1",
    ]).then((r) => r.value ?? []);
    typeStates.set(key, load);
    load.catch(() => typeStates.delete(key));
  }
  return typeStates.get(key);
}

function parentTitle(org, id) {
  const key = `${org} ${id}`;
  if (!parents.has(key)) {
    const load = show(org, id).then((p) => p.fields["System.Title"] ?? "");
    parents.set(key, load);
    load.catch(() => parents.delete(key));
  }
  return parents.get(key);
}

function refOf(key) {
  return [...refs.values()].find((r) => r.key === key);
}

async function refreshItem(key, { byUser = false } = {}) {
  const ref = refOf(key);
  if (!ref) return;
  if (byUser) manual.add(key);
  if (inFlight.has(key)) return;
  inFlight.add(key);
  render();
  try {
    items.set(key, await loadItem(ref));
  } catch (error) {
    const message = String(error.message ?? error);
    items.set(key, NOT_FOUND.test(message) ? { missing: true } : { error: message });
  } finally {
    inFlight.delete(key);
    manual.delete(key);
    render();
  }
}

async function loadItem({ org, id }) {
  const wi = await show(org, id);
  const f = wi.fields;
  const project = f["System.TeamProject"];
  const type = f["System.WorkItemType"];
  const [states, parent] = await Promise.all([
    statesOf(org, project, type).catch(() => []),
    f["System.Parent"] ? parentTitle(org, f["System.Parent"]).catch(() => "") : null,
  ]);
  const iteration = f["System.IterationPath"] ?? "";
  return {
    id: wi.id,
    title: f["System.Title"],
    state: f["System.State"],
    type,
    category: states.find((s) => s.name === f["System.State"])?.category,
    states,
    sprint: iteration.includes("\\") ? iteration.split("\\").at(-1) : "None",
    assigned: f["System.AssignedTo"]?.displayName ?? "Unassigned",
    parent: f["System.Parent"] ? `#${f["System.Parent"]} ${parent}`.trim() : null,
    // A visualstudio.com org also answers at dev.azure.com, the one URL prefix the manifest allows.
    url: `${org.replace(/^https:\/\/([^.]+)\.visualstudio\.com$/, "https://dev.azure.com/$1")}/${encodeURIComponent(project)}/_workitems/edit/${wi.id}`,
  };
}

/** Process categories: Proposed, InProgress, Resolved, Completed, Removed. Review and QA states wait on someone else. */
function toneOf(item) {
  if (item.category === "InProgress") return /review|qa|test/i.test(item.state) ? "warning" : "info";
  return { Resolved: "merged", Completed: "success" }[item.category] ?? "neutral";
}

/** The next two states in the type's workflow order, skipping Removed. */
function nextStates(item) {
  const forward = item.states.filter((s) => s.category !== "Removed");
  const at = forward.findIndex((s) => s.name === item.state);
  if (at < 0) return [];
  return forward.slice(at + 1, at + 3).map((s) => s.name).filter((name) => name.length <= 32);
}

/** The one write: a failure shows in the card, and doesn't count as a sign-in problem for the org. */
async function moveTo(ref, state) {
  manual.add(ref.key);
  render();
  const out = await wings.exec("az", ["boards", "work-item", "update", "--id", ref.id, "--state", state, "--organization", ref.org, "-o", "json"]);
  if (out.code !== 0) {
    manual.delete(ref.key);
    render();
    throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
  }
  return refreshItem(ref.key, { byUser: true });
}

// ---------- badge ----------

function badgeFor(ref, item) {
  const actions = [
    { id: "open", label: "Open in Azure DevOps", primary: true },
    { id: "refresh", label: "Refresh" },
  ];
  const label = `#${ref.id}`;
  if (!item) return { label, tone: "neutral", title: "Loading work item…", subtitle: `Work item #${ref.id}`, actions };
  if (item.error) return { label, tone: "neutral", title: `Work item #${ref.id}`, rows: [{ label: "Status", value: item.error, tone: "danger" }], actions };
  const tone = toneOf(item);
  return {
    label: `#${ref.id} ${item.state}`,
    tone,
    title: item.title,
    subtitle: `${item.type} #${ref.id}`,
    rows: [
      { label: "Type", value: item.type },
      { label: "State", value: item.state, tone },
      { label: "Sprint", value: item.sprint },
      { label: "Assigned", value: item.assigned },
      ...(item.parent ? [{ label: "Parent", value: item.parent }] : []),
    ],
    actions: [...actions, ...nextStates(item).map((s) => ({ id: `move:${s}`, label: `Move to ${s}` }))],
  };
}

function problemBadge(problem) {
  if (!problem.login) {
    return {
      label: "Work item unavailable",
      tone: "warning",
      title: "Azure DevOps",
      rows: [{ label: "Error", value: problem.message, tone: "warning" }],
      actions: [{ id: "retry", label: "Try again", primary: true }],
    };
  }
  return {
    label: signingIn ? "Signing in…" : "Sign in to Azure DevOps",
    tone: "warning",
    loading: signingIn,
    title: "Azure DevOps",
    subtitle: signingIn ? "Finish signing in in your browser." : problem.message,
    actions: [{ id: "signin", label: "Sign in", primary: true }, { id: "retry", label: "Try again" }],
  };
}

function render() {
  for (const pane of panes) {
    const ref = refs.get(pane.paneId);
    const item = ref && items.get(ref.key);
    const problem = ref && problems.get(ref.org);
    if (problem?.login || (problem && !item?.title)) {
      void wings.setBadge(pane.paneId, problemBadge(problem));
    } else if (ref && !item?.missing) {
      const loading = manual.has(ref.key) || (inFlight.has(ref.key) && !item);
      void wings.setBadge(pane.paneId, { ...badgeFor(ref, item), loading });
    } else {
      void wings.setBadge(pane.paneId, null);
    }
  }
}

async function signIn() {
  if (signingIn) return;
  signingIn = true;
  render();
  try {
    const out = await wings.exec("az", ["login", "--allow-no-subscriptions", "--output", "none"], { timeoutMs: SIGN_IN_TIMEOUT_MS });
    if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "Sign-in failed");
  } finally {
    signingIn = false;
  }
  return retry();
}

function retry() {
  problems.clear();
  items.clear();
  parents.clear();
  typeStates.clear();
  return refreshRefs();
}

wings.onAction(async ({ paneId, actionId }) => {
  if (actionId === "signin") return signIn();
  if (actionId === "retry") return retry();
  const ref = refs.get(paneId);
  if (!ref) return;
  if (actionId === "refresh") return refreshItem(ref.key, { byUser: true });
  const item = items.get(ref.key);
  if (actionId === "open" && item?.url) return wings.openUrl(item.url);
  if (actionId.startsWith("move:")) {
    const state = actionId.slice("move:".length);
    if (!item?.states || !nextStates(item).includes(state)) throw new Error(`${state} isn't a next state of this work item any more.`);
    return moveTo(ref, state);
  }
});
