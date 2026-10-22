import test from "node:test";
import assert from "node:assert/strict";
import { sessionStartText, staleIssues } from "./session-start.ts";

test("the session start line names the linked issue and the steps left, or asks for an issue", () => {
  const issue = { key: "WEB-4", title: "Checkout summary", status: "in_progress" };
  const linked = sessionStartText({ projectName: "web", keepLocal: true, issue, todoLeft: 3 });
  assert.match(linked, /WEB-4 "Checkout summary" \(In progress\)/);
  assert.match(linked, /3 steps left; tasks__todo with no items reads it/);
  const fresh = sessionStartText({ projectName: "web", keepLocal: true });
  assert.match(fresh, /^When a task will change files/m);
  assert.match(fresh, /A question, a check, a review or a lookup gets no issue/);
  assert.match(fresh, /tasks__list or create one with tasks__create/);
  assert.doesNotMatch(fresh, /steps? left/);
  assert.doesNotMatch(sessionStartText({ projectName: "web", keepLocal: false }), /never go in commits/);
});

test("an issue in progress or in review is listed as stale from 3 days, not at 2, and a finished todo is named", () => {
  const DAY = 24 * 60 * 60 * 1000;
  const now = 100 * DAY;
  const issue = (key, status, days) => ({ key, title: key, status, updatedAt: now - days * DAY, sessions: [] });
  const issues = [issue("WEB-1", "in_progress", 5), issue("WEB-2", "in_progress", 2), issue("WEB-3", "in_review", 3), issue("WEB-4", "done", 9), issue("WEB-5", "todo", 9)];
  assert.deepEqual(staleIssues(issues, now).map((i) => i.key), ["WEB-1", "WEB-3"]);
  const text = sessionStartText({ projectName: "web", keepLocal: true, stale: staleIssues(issues, now), now });
  assert.match(text, /WEB-1 is In progress but unchanged for 5 days\./);
  assert.match(text, /WEB-3 is In review but unchanged for 3 days\./);
  assert.doesNotMatch(text, /WEB-2|WEB-4|WEB-5/);
  const two = sessionStartText({ projectName: "web", keepLocal: true, stale: staleIssues([issue("WEB-2", "in_progress", 2)], now), now });
  assert.doesNotMatch(two, /unchanged/);
  const many = staleIssues(Array.from({ length: 5 }, (_, i) => issue(`WEB-${i}`, "in_progress", 4 + i)), now);
  assert.match(sessionStartText({ projectName: "web", keepLocal: true, stale: many, now }), /And 2 more\.$/);

  const finished = sessionStartText({ projectName: "web", keepLocal: true, issue: issue("WEB-1", "in_progress", 0), finished: issue("WEB-1", "in_progress", 0), stale: [issue("WEB-1", "in_progress", 6)], now });
  assert.match(finished, /Your todo is all done, but WEB-1 is still In progress: worth checking whether it is finished\./);
  assert.doesNotMatch(finished, /unchanged/, "the finished issue is not listed twice");
  assert.doesNotMatch(sessionStartText({ projectName: "web", keepLocal: true }), /Your todo is all done|Worth checking/);
});
