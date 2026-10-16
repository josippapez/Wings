// The Sprint block of a Project page: a sprint running, nothing assigned to you, and what goes wrong.
import { sprint } from "./_data";

export { default, page } from "../src/pages/sprint";

export const states = {
  // A sprint running, Mine first.
  block: { queries: { sprint: () => sprint } },
  // Nothing is assigned to you in the running sprint.
  empty: { queries: { sprint: () => ({ ...sprint, items: [], total: 0 }) } },
  // Azure DevOps could not be read.
  error: {
    queries: {
      sprint: (): never => {
        throw new Error("az boards query failed: Please run 'az login' to set up an account.");
      },
    },
  },
  // Before the main script has answered.
  loading: {},
  // Wings has no project to say.
  "no-project": { project: null },
};
