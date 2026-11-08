// The page hooks ui.js and native.js share: the same React state over the same `wings` calls, whichever app draws the page.
import { createElement as h, Fragment, useCallback, useEffect, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";

/**
 * The `ProjectGate` of a module, over its own `useProject`, `Loading` and `Empty`. It shows `children(project)` once the
 * page knows its project, keyed by the project's id, so a page that holds state starts over for another project. Until
 * then it shows `loading`. With no project it shows an `Empty` titled "No project", unless `empty` says otherwise.
 */
export const projectGate = (useProject, Loading, Empty) =>
  function ProjectGate({ loading = "Loading…", empty, children }) {
    const project = useProject();
    if (project === undefined) return h(Loading, null, loading);
    if (project === null) return h(Empty, { title: "No project", ...empty });
    return h(Fragment, { key: project.id }, children(project));
  };

/** A saved value, read once: null until it is, then what `check` makes of it, or `initial` when nothing was saved or `check` says no. */
function useSaved(read, write, initial, check) {
  const [value, setValue] = useState(null);
  useEffect(() => {
    void read()
      .catch(() => null)
      .then((raw) => setValue((raw == null ? undefined : check(raw)) ?? initial));
    // Read once, when the component mounts.
  }, []);
  // A save that fails, like one that is too big, leaves the page with the value it set.
  return [
    value,
    (next) => {
      setValue(next);
      write(next).catch(() => {});
    },
  ];
}

/**
 * The state the tab saved with `wings.setTabState`, and a function that sets and saves it. The value is null until it's
 * read, then what `check` returns for the saved state, or `initial` when there is none or `check` returns null or undefined.
 */
export const useTabState = (initial, check) => useSaved(() => wings.tabState(), (state) => wings.setTabState(state), initial, check);
/** Like `useTabState`, for a block: the state saved with `wings.setBlockState`. */
export const useBlockState = (initial, check) => useSaved(() => wings.blockState(), (state) => wings.setBlockState(state), initial, check);

/**
 * What `wings.openView` opened this tab with, as `check` reads it, or null when there were no args or `check` returns
 * null or undefined for them. Read once: a tab opened with other args reloads.
 */
export function useViewArgs(check) {
  const [args] = useState(() => {
    const raw = wings.viewArgs();
    return raw == null ? null : (check(raw) ?? null);
  });
  return args;
}

/**
 * Runs the page's async actions and keeps what its buttons and error card show: `busy` while any `run` is going, and
 * the message of the last one that threw as `error`. Starting a `run` clears the error; `clear` does it by hand. `run`
 * says whether the action worked, so a caller that closes an editor or clears a draft can do it only then.
 */
export function useAction() {
  const [running, setRunning] = useState(0);
  const [error, setError] = useState(null);
  const run = useCallback(async (fn) => {
    setRunning((n) => n + 1);
    setError(null);
    try {
      await fn();
      return true;
    } catch (e) {
      setError(errorMessage(e));
      return false;
    } finally {
      setRunning((n) => n - 1);
    }
  }, []);
  const clear = useCallback(() => setError(null), []);
  return { run, busy: running > 0, error, clear };
}
