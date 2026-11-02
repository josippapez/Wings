// "Allow this schedule?": an agent asks to add a schedule, asks to change one's prompt and time with another waiting, and
// nothing left to answer.
import type { Approval } from "../src/lib/approval";

export { default, page } from "../src/pages/approve";

const add: Approval = {
  id: "1",
  asker: "Discussion check",
  projectName: "acme-web",
  current: null,
  proposed: {
    name: "",
    projectId: "p1",
    agent: "claude",
    prompt: "Check the repo's open discussions for comments newer than the last run, summarise each in one or two lines, and draft replies for me. Don't post anything.",
    when: { kind: "daily", time: "09:00", days: [0, 1, 2, 3, 4, 5, 6] },
  },
};

const change: Approval = {
  id: "2",
  asker: null,
  projectName: "acme-api",
  current: { name: "Morning review", projectId: "p2", agent: "claude", prompt: "Read yesterday's merged pull requests and list anything that needs a follow-up.", when: { kind: "daily", time: "09:00", days: [1, 2, 3, 4, 5] } },
  proposed: { name: "Morning review", projectId: "p2", agent: "claude", prompt: "Read yesterday's merged pull requests, list anything that needs a follow-up and open a fix for the first one.", when: { kind: "daily", time: "08:30", days: [1, 2, 3, 4, 5] } },
};

export const states = {
  add: { queries: { approvals: [add] } },
  change: { queries: { approvals: [change, add] } },
  empty: { queries: { approvals: [] } },
};
