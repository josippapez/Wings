/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";

import { MAX_NOTIFICATIONS, notificationsReducer, unreadCount, type NotificationEvent } from "./notifications.ts";

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
