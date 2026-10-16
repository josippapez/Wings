// What this plugin's main script and its pages say to each other. Both import `ch`.

import { channel } from "wings-plugin-sdk/messages";
import type { BlockItem } from "./sections";

/** The Sprint block shows the sprint of the Azure DevOps project that the folder `path` is in. `mine`: only what's assigned to you; false is the whole sprint. */
export interface SprintRequest {
  path: string;
  mine: boolean;
}

/** The running sprints of an Azure DevOps project and their work items: `items` is empty with no running sprint, and `total` counts past the cap. */
export interface Sprint {
  org: string;
  project: string;
  sprints: string[];
  items: BlockItem[];
  total: number;
}

/** `none` is a project that isn't on Azure DevOps. */
export type SprintAnswer = Sprint | { none: true };

export const ch = channel<
  {},
  // The item tab moved a work item to another state.
  { "item-changed": { org: string; id: number } },
  { sprint: { key: SprintRequest; value: SprintAnswer } }
>();
