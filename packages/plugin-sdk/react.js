import { useCallback, useEffect, useRef, useState } from "react";
import { pollWhileFocused } from "./poll.js";

/**
 * Calls `listener` with each message the plugin's other pages send with `wings.broadcast`, while the component
 * is mounted. The latest `listener` is used, so it can read state without resubscribing.
 */
export function useBroadcast(listener) {
  const latest = useRef(listener);
  latest.current = listener;
  useEffect(() => wings.onBroadcast((message) => latest.current(message)), []);
}

// What `useAsk` holds before its first answer, shared so setting it on mount changes nothing.
const IDLE = { data: null, error: null, loading: false };
const LOADING = { data: null, error: null, loading: true };

/**
 * Asks `name` on the channel when the component mounts and whenever `request` changes (compared as JSON), and every
 * `pollMs` while Wings is focused. `data` is the latest answer, kept while the next is on its way, and `null` once the
 * request changes. `error` is the last ask's error, cleared by the next answer. An answer to an ask that a newer one
 * replaced is ignored. `skip` asks nothing.
 */
export function useAsk(ch, name, request, { pollMs, skip = false } = {}) {
  const [state, setState] = useState(skip ? IDLE : LOADING);
  const ask = useRef(() => {});
  const refresh = useCallback(() => ask.current(), []);
  const latest = useRef(request);
  latest.current = request;
  const key = JSON.stringify(request);
  useEffect(() => {
    setState(skip ? IDLE : LOADING);
    ask.current = () => {};
    if (skip) return;
    let live = true;
    let turn = 0;
    ask.current = () => {
      const mine = ++turn;
      setState((s) => (s.loading ? s : { ...s, loading: true }));
      ch.ask(name, latest.current).then(
        (data) => live && mine === turn && setState({ data, error: null, loading: false }),
        (error) => live && mine === turn && setState((s) => ({ ...s, error, loading: false })),
      );
    };
    ask.current();
    const stop = pollMs ? pollWhileFocused(() => ask.current(), pollMs) : undefined;
    return () => {
      live = false;
      stop?.();
    };
  }, [ch, name, key, pollMs, skip]);
  return { ...state, refresh };
}

/** The state main shares as `name` on the channel, or `null` until main has sent it. */
export function useShared(ch, name) {
  const [state, setState] = useState(null);
  useEffect(() => {
    setState(null);
    return ch.watch(name, setState);
  }, [ch, name]);
  return state;
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
