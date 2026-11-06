import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { git } from "wings-plugin-sdk/git";
import { Button, Empty, Graph, Notice, Page, Spinner, Text, Toolbar } from "wings-plugin-ui/native";
import { layout, type Placed } from "../lib/graph-layout";
import { LOG, parseLog } from "../lib/log";
import { ch } from "../lib/messages";
import { dateOf } from "../lib/dates";

export function GraphNative() {
  const [repo, setRepo] = useState<{ root: string | null; name: string | null }>({ root: null, name: null });
  const [history, setHistory] = useState<{ rows: Placed[]; lanes: number }>({ rows: [], lanes: 1 });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [selected, setSelected] = useState(-1);
  const [range, setRange] = useState({ start: 0, end: 64 });
  const generation = useRef(0);
  const shownRoot = useRef<string | null>(null);
  const load = useCallback(async () => {
    const own = ++generation.current;
    setLoading(true);
    setError(undefined);
    try {
      const next = await ch.ask("graph-repo");
      if (generation.current !== own) return;
      if (shownRoot.current !== next.root) {
        shownRoot.current = next.root;
        setHistory({ rows: [], lanes: 1 });
        setSelected(-1);
        setRange({ start: 0, end: 64 });
      }
      setRepo(next);
      const placed = next.root ? layout(parseLog(await git(next.root, ["log", "--all", "--topo-order", ...LOG, "--max-count=10000"], { raw: true }))) : { rows: [], lanes: 1 };
      if (generation.current !== own) return;
      setHistory(placed);
      setSelected(-1);
    } catch (e) {
      if (generation.current === own) setError(errorMessage(e));
    } finally {
      if (generation.current === own) setLoading(false);
    }
  }, []);
  useEffect(() => {
    void load();
    const off = ch.on("graph-repo-changed", () => void load());
    return () => { generation.current++; off(); };
  }, [load]);
  const select = (index: number) => {
    const c = history.rows[index];
    if (!c || !repo.root) return;
    setSelected(index);
    void ch.ask("commit", { root: repo.root, hash: c.hash, subject: c.subject, author: c.author, at: c.at }).catch((e) => setError(errorMessage(e)));
  };
  const total = history.rows.length;
  const offset = Math.min(range.start, total);
  return <Page flush toolbar={<Toolbar title={repo.name ?? "Commit graph"} actions={<>{loading ? <Spinner label="Loading" /> : null}<Button variant="outline" disabled={loading || !repo.root} onClick={load}>Refresh</Button></>}>
    <Text tone="muted">{loading ? "Loading…" : `${total.toLocaleString()} commits on every branch${total >= 10000 ? ", the newest 10,000" : ""}`}</Text>
  </Toolbar>}>
    {error ? <Notice tone="warn">{error}</Notice> : null}
    {total ? <Graph total={total} lanes={history.lanes} selected={selected} offset={offset}
      rows={history.rows.slice(offset, Math.min(range.end, offset + 256)).map((c) => ({ hash: c.hash, subject: c.subject, author: c.author, date: dateOf(c.at), refs: c.refs, lane: c.lane, color: c.color, merge: c.parents.length > 1, lines: c.lines }))}
      onRangeChange={(next) => setRange((old) => old.start === next.start && old.end === next.end ? old : next)} onSelect={select} />
      : !loading ? <Empty title={repo.root ? "No commits yet." : "No git repo in your panes."} /> : null}
  </Page>;
}
