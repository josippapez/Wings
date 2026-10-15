// How issues relate: parents and children, related and blocking links. The queries read them, `relate` changes them.

import { canParent, isType, KEY_PATTERN, keyNumber, MAX_LINKS, normalizeKey, TYPE_LABEL, TYPES, type Actor, type Issue, type LogKind, type Relations } from "./domain";
import { append } from "./log";

function keyList(value: unknown, field: string): string[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.some((v) => typeof v !== "string")) throw new Error(`${field} must be a list of issue keys.`);
  return [...new Set((value as string[]).map(normalizeKey))].filter(Boolean);
}

/** The issues whose parent is `key`. */
export const childrenOf = (issues: Issue[], key: string): Issue[] => issues.filter((i) => i.parent === key);
/** The issues under `key` at any depth: its children, theirs and so on. */
export function descendantsOf(issues: Issue[], key: string): Issue[] {
  const below = new Set<string>([key]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const i of issues) if (i.parent && below.has(i.parent) && !below.has(i.key)) (below.add(i.key), (grew = true));
  }
  below.delete(key);
  return issues.filter((i) => below.has(i.key));
}
/** The issues `key` is blocking: the ones that list it as blocking them. */
export const blocksOf = (issues: Issue[], key: string): Issue[] => issues.filter((i) => i.blockedBy?.includes(key));
/** Done out of all, not counting canceled ones: "3 of 7 done". */
export function progress(children: Issue[]): { done: number; total: number } {
  const live = children.filter((c) => c.status !== "canceled");
  return { done: live.filter((c) => c.status === "done").length, total: live.length };
}
/** The epics and features `issue` may sit under: not itself, and not one of its own children, grandchildren and so on. */
export function parentChoices(issues: Issue[], issue: Issue): Issue[] {
  const below = new Set<string>([issue.key]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const i of issues) if (i.parent && below.has(i.parent) && !below.has(i.key)) (below.add(i.key), (grew = true));
  }
  return issues.filter((i) => canParent(i.type) && !below.has(i.key)).sort((a, b) => keyNumber(a.key) - keyNumber(b.key));
}

/**
 * Applies the type and link changes of an op to `self`, a copy in `map`, and to the other issues it touches, which are
 * copies in `map` too. Every change writes an entry on both issues. Returns whether anything changed.
 */
export function relate(map: Map<string, Issue>, self: Issue, op: Relations, by: Actor, now: number): boolean {
  let changed = false;
  const log = (issue: Issue | undefined, kind: LogKind, text: string) => {
    if (!issue) return;
    append(issue, [{ at: now, by, kind, text }]);
    if (issue !== self) issue.updatedAt = now;
  };
  const other = (raw: string): Issue => {
    if (!KEY_PATTERN.test(raw)) throw new Error(`${raw} isn't an issue key, like WIN-12.`);
    if (raw === self.key) throw new Error(`${self.key} can't link to itself.`);
    const found = map.get(raw);
    if (!found) throw new Error(`No issue ${raw} in this project. Use list to see the keys.`);
    return found;
  };
  const without = (list: string[] | undefined, key: string) => {
    const rest = (list ?? []).filter((k) => k !== key);
    return rest.length ? rest : undefined;
  };
  const set = (issue: Issue, field: "related" | "blockedBy", list: string[] | undefined) => {
    if (list) issue[field] = list;
    else delete issue[field];
  };

  if (op.type !== undefined) {
    if (!isType(op.type)) throw new Error(`type must be one of ${TYPES.join(", ")}.`);
    if (op.type !== self.type) {
      if (!canParent(op.type) && [...map.values()].some((i) => i.parent === self.key)) throw new Error(`${self.key} has child issues, so it has to stay an epic or a feature. Move them to another parent first.`);
      log(self, "type", `Type: ${TYPE_LABEL[self.type]} → ${TYPE_LABEL[op.type]}`);
      self.type = op.type;
      changed = true;
    }
  }

  if (op.parent !== undefined) {
    if (typeof op.parent !== "string") throw new Error("parent must be an issue key, or an empty string to clear it.");
    const wanted = normalizeKey(op.parent);
    if (!wanted) {
      if (self.parent) {
        log(self, "link", `Parent removed (was ${self.parent})`);
        log(map.get(self.parent), "link", `Child removed: ${self.key}`);
        delete self.parent;
        changed = true;
      }
    } else if (wanted !== self.parent) {
      const parent = other(wanted);
      if (!canParent(parent.type)) throw new Error(`${parent.key} is a ${TYPE_LABEL[parent.type].toLowerCase()}. Only an epic or a feature can be a parent: change its type first.`);
      const seen = new Set<string>();
      for (let up: Issue | undefined = parent; up && !seen.has(up.key); up = up.parent ? map.get(up.parent) : undefined) {
        if (up.key === self.key) throw new Error(`${parent.key} is under ${self.key} already, so ${self.key} can't go under it.`);
        seen.add(up.key);
      }
      if (self.parent) log(map.get(self.parent), "link", `Child removed: ${self.key}`);
      log(self, "link", `Parent: ${parent.key}`);
      log(parent, "link", `Child: ${self.key}`);
      self.parent = parent.key;
      changed = true;
    }
  }

  for (const raw of keyList(op.relatedAdd, "related_add")) {
    const peer = other(raw);
    if (self.related?.includes(raw)) continue;
    if ((self.related?.length ?? 0) >= MAX_LINKS) throw new Error(`${self.key} already has ${MAX_LINKS} related issues, the most one keeps.`);
    if ((peer.related?.length ?? 0) >= MAX_LINKS) throw new Error(`${peer.key} already has ${MAX_LINKS} related issues, the most one keeps.`);
    self.related = [...(self.related ?? []), raw];
    peer.related = [...(peer.related ?? []), self.key];
    log(self, "link", `Linked to ${raw} as related`);
    log(peer, "link", `Linked to ${self.key} as related`);
    changed = true;
  }
  for (const raw of keyList(op.relatedRemove, "related_remove")) {
    if (!self.related?.includes(raw)) continue;
    set(self, "related", without(self.related, raw));
    const peer = map.get(raw);
    if (peer) set(peer, "related", without(peer.related, self.key));
    log(self, "link", `Unlinked from ${raw} (related)`);
    log(peer, "link", `Unlinked from ${self.key} (related)`);
    changed = true;
  }

  for (const raw of keyList(op.blockedByAdd, "blocked_by_add")) {
    const blocker = other(raw);
    if (self.blockedBy?.includes(raw)) continue;
    if ((self.blockedBy?.length ?? 0) >= MAX_LINKS) throw new Error(`${self.key} is already blocked by ${MAX_LINKS} issues, the most one keeps.`);
    // A blocker that waits on this issue, however far down the chain, would leave both stuck.
    const seen = new Set<string>();
    const stack = [blocker];
    for (let next = stack.pop(); next; next = stack.pop()) {
      if (seen.has(next.key)) continue;
      seen.add(next.key);
      if (next.key === self.key) throw new Error(`${raw} is waiting on ${self.key} already, so ${self.key} can't wait on it: they would block each other.`);
      for (const k of next.blockedBy ?? []) if (map.has(k)) stack.push(map.get(k)!);
    }
    self.blockedBy = [...(self.blockedBy ?? []), raw];
    log(self, "link", `Blocked by ${raw}`);
    log(blocker, "link", `Blocks ${self.key}`);
    changed = true;
  }
  for (const raw of keyList(op.blockedByRemove, "blocked_by_remove")) {
    if (!self.blockedBy?.includes(raw)) continue;
    set(self, "blockedBy", without(self.blockedBy, raw));
    log(self, "link", `No longer blocked by ${raw}`);
    log(map.get(raw), "link", `No longer blocks ${self.key}`);
    changed = true;
  }
  return changed;
}
