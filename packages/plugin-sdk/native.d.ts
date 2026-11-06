/** Experimental browser-free Issues contract. Data only, with no HTML, callbacks or browser SDK compatibility. */
export interface NativeProject {
  id: string;
  name: string;
}

export type NativeAction = null | "refresh";
export type NativeStatus = "todo" | "in_progress" | "in_review" | "done" | "canceled";
export type NativeIssueType = "task" | "bug" | "feature" | "epic";

export interface NativeRow {
  /** The existing issue key, at most 20 characters. */
  id: string;
  /** At most 200 UTF-16 code units, matching Issues' title limit. */
  title: string;
  status: NativeStatus;
  type: NativeIssueType;
}

export interface NativeScreen {
  version: 1;
  /** At most 200 UTF-16 code units. */
  title: string;
  state: "ready" | "empty" | "error";
  /** At most 2,000 rows. An overflow returns an error, never a partial list. */
  rows: NativeRow[];
  /** Exactly one Refresh action. Its label is at most 200 UTF-16 code units. */
  actions: [{ id: "refresh"; label: string }];
  /** Present on an error screen, at most 500 UTF-16 code units. */
  error?: string;
}

/** The worker freezes this read-only host for each invocation. There are no write or browser methods. */
export interface NativeHost {
  readonly storage: {
    get(key: string): Promise<unknown | null>;
    entries(prefix: string): Promise<[string, unknown][]>;
  };
}

/** Installed on globalThis.wingsNative by the experimental IIFE. Nothing runs until the host invokes it. */
export type NativeEntrypoint = (project: NativeProject, action: NativeAction) => Promise<NativeScreen>;

declare global {
  var wingsNative: NativeEntrypoint;
}
