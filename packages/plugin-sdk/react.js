import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/**
 * Calls `listener` with each message the plugin's other pages send with `wings.broadcast`, while the component
 * is mounted. The latest `listener` is used, so it can read state without resubscribing.
 */
export function useBroadcast(listener) {
  const latest = useRef(listener);
  latest.current = listener;
  useEffect(() => wings.onBroadcast((message) => latest.current(message)), []);
}

const TIMEOUT = "Wings didn't answer. Try again.";
const IDLE = { data: null, error: null, loading: false, at: 0 };

/**
 * The value of the query `name` for `key`, which main serves on the channel, and its updates. Components in one frame
 * showing one key share one subscription. `loading` is true before the first value for a key and while `refresh()` waits;
 * main's own refetches never set it. `keepData` keeps the previous key's value while a new key loads. With no value or
 * error after `timeoutMs` (default 30 s), `error` says Wings didn't answer, and a value that comes later clears it.
 * `skip` subscribes to nothing.
 */
export function useQuery(ch, name, key, { skip = false, keepData = false, timeoutMs = 30_000 } = {}) {
  const id = skip ? null : JSON.stringify([name, key ?? null]);
  const latestKey = useRef(key);
  latestKey.current = key;
  const [state, setState] = useState(() => (skip ? IDLE : { ...IDLE, loading: true }));
  const waiting = useRef(undefined);

  useEffect(() => {
    if (id === null) {
      setState(IDLE);
      return;
    }
    setState((s) => ({ data: keepData ? s.data : null, error: null, loading: true, at: 0 }));
    const timeout = () => setState((s) => (s.loading ? { ...s, error: TIMEOUT, loading: false } : s));
    const timer = setTimeout(timeout, timeoutMs);
    const off = ch.subscribe(name, latestKey.current, (snap) => {
      if (!snap.loading) {
        clearTimeout(timer);
        clearTimeout(waiting.current);
      }
      setState((s) => ({ data: snap.data ?? (keepData && snap.error === null ? s.data : null), error: snap.error, loading: snap.loading, at: snap.at }));
    });
    waiting.current = undefined;
    return () => {
      off();
      clearTimeout(timer);
      clearTimeout(waiting.current);
    };
  }, [ch, id, keepData, timeoutMs]);

  const refresh = useCallback(() => {
    if (id === null) return;
    ch.refresh(name, latestKey.current ?? null);
    clearTimeout(waiting.current);
    waiting.current = setTimeout(() => setState((s) => (s.loading ? { ...s, error: TIMEOUT, loading: false } : s)), timeoutMs);
  }, [ch, id, timeoutMs]);

  const error = useMemo(() => (state.error === null ? null : new Error(state.error)), [state.error]);
  return { data: state.data, error, loading: state.loading, at: state.at, refresh };
}

/** A setting's value, `null` until it is read, and a function that sets it. It follows changes made in any frame. */
export function useSetting(setting) {
  const [value, setValue] = useState(null);
  useEffect(() => {
    let live = true;
    void setting.get().then((v) => live && setValue(v));
    const off = setting.onChange((v) => live && setValue(v));
    return () => {
      live = false;
      off();
    };
  }, [setting]);
  const set = useCallback(
    (v) => {
      setValue(v);
      return setting.set(v);
    },
    [setting],
  );
  return [value, set];
}
