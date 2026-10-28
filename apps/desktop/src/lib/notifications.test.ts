/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { cleanButtons, MAX_NOTIFICATIONS, notificationsReducer, restoreNotifications, shouldToastPlugin, unreadCount, unreadPluginCount, type NotificationEvent } from "./notifications.ts";

const event = (id: string, over: Partial<NotificationEvent> = {}): NotificationEvent => ({
  id, at: Number(id) || 0, kind: "done", title: "Claude finished", project: "p", detail: "", paneId: "pane", read: false, ...over,
});
const add = (list: NotificationEvent[], e: NotificationEvent) => notificationsReducer(list, { type: "add", event: e });

test("the newest event comes first", () => {
  const list = add(add([], event("1")), event("2"));
  assert.deepEqual(list.map((e) => e.id), ["2", "1"]);
});

test("keeps the last 100 and drops the oldest", () => {
  let list: NotificationEvent[] = [];
  for (let i = 1; i <= MAX_NOTIFICATIONS + 5; i++) list = add(list, event(String(i)));
  assert.equal(list.length, 100);
  assert.equal(list[0].id, "105");
  assert.equal(list.at(-1)!.id, "6");
});

test("an id that is already listed is not added twice", () => {
  const list = add([], event("1"));
  assert.equal(add(list, event("1")), list);
});

test("new events are unread and reading one lowers the count", () => {
  let list = add(add(add([], event("1")), event("2")), event("3", { read: true }));
  assert.equal(unreadCount(list), 2);
  list = notificationsReducer(list, { type: "read", id: "1" });
  assert.equal(unreadCount(list), 1);
  assert.equal(list.find((e) => e.id === "1")!.read, true);
});

test("reading an unknown or already read event returns the same list", () => {
  const list = add([], event("1", { read: true }));
  assert.equal(notificationsReducer(list, { type: "read", id: "1" }), list);
  assert.equal(notificationsReducer(list, { type: "read", id: "nope" }), list);
  assert.equal(notificationsReducer(list, { type: "readAll" }), list);
});

test("mark all read keeps every event", () => {
  const list = notificationsReducer(add(add([], event("1")), event("2")), { type: "readAll" });
  assert.equal(list.length, 2);
  assert.equal(unreadCount(list), 0);
});

test("clear empties the list", () => {
  assert.deepEqual(notificationsReducer(add([], event("1")), { type: "clear" }), []);
});

test("a plugin's notification buttons open its tab views or an https link, three at most", () => {
  const views = ["pr"];
  assert.deepEqual(cleanButtons(undefined, views), []);
  assert.deepEqual(
    cleanButtons([{ label: " Open ", open: { view: "pr", args: { n: 1 } } }, { label: "Open in browser", url: "https://example.com/pr/1" }, { label: "All", open: { view: "pr" } }, { label: "Extra", url: "https://example.com" }], views),
    [{ label: "Open", open: { view: "pr", args: { n: 1 } } }, { label: "Open in browser", url: "https://example.com/pr/1" }, { label: "All", open: { view: "pr" } }],
  );
  assert.equal(cleanButtons([{ label: "x".repeat(40), url: "https://a.b" }], views)[0].label.length, 32);
});

test("a button that opens something the plugin can't is refused", () => {
  const views = ["pr"];
  assert.throws(() => cleanButtons("open", views), /must be a list/);
  assert.throws(() => cleanButtons([{ url: "https://a.b" }], views), /needs a label/);
  assert.throws(() => cleanButtons([{ label: "Go", url: "http://a.b" }], views), /https link/);
  assert.throws(() => cleanButtons([{ label: "Go", open: { view: "settings" } }], views), /isn't one of the plugin's views/);
  assert.throws(() => cleanButtons([{ label: "Go", open: { view: "pr", args: "x".repeat(20_000) } }], views), /16 KB|too big|over/);
});

test("the saved list comes back, and Go to pane only for a pane that still runs", () => {
  const saved = [event("2", { paneId: "p1" }), event("1", { paneId: "p7", read: true }), { id: "bad" }, null];
  const list = restoreNotifications(JSON.parse(JSON.stringify(saved)), new Set(["p7"]));
  assert.deepEqual(list.map((e) => [e.id, e.paneId, e.read]), [["2", null, false], ["1", "p7", true]]);
  assert.deepEqual(restoreNotifications(undefined, new Set()), []);
  assert.deepEqual(restoreNotifications("x", new Set()), []);
});

test("a plugin notification toasts only while Wings is in front", () => {
  assert.equal(shouldToastPlugin(true), true);
  assert.equal(shouldToastPlugin(false), false);
});

test("counts only unread plugin events", () => {
  const list = [event("1", { kind: "plugin" }), event("2", { kind: "plugin", read: true }), event("3", { kind: "done" })];
  assert.equal(unreadPluginCount(list), 1);
});
