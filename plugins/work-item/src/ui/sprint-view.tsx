// The Sprint block of a Project page: the work items of the sprint running now in the project's Azure DevOps
// project, a column per state. The main script runs the queries and caches them; this page asks it, only while
// Wings is in front and the block is on screen, and a click opens the work item in a tab.

import { useEffect, useRef, useState } from "react";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { Avatar, Board, BoardColumn, Button, Card, Chips, Empty, Loading, Row, Stack, Text } from "wings-plugin-ui";
import { cleanBlockState, groupByState, type BlockItem, type SprintBlockState, type SprintRequest, type SprintResponse } from "../lib/sections";
import { TypeIcon } from "../lib/TypeIcon";
import "../sprint.css";

const POLL_MS = 60_000;

type Answer = Omit<SprintResponse, "type" | "rid">;

/** The project's sprint: once at the start and when `mine` changes, then on each poll while Wings is focused. A hidden block asks for nothing. */
function useSprint(path: string, mine: boolean): Answer | null {
  const [answer, setAnswer] = useState<Answer | null>(null);
  useEffect(() => {
    let pending = "";
    const ask = () => {
      if (document.visibilityState === "hidden") return;
      pending = crypto.randomUUID();
      wings.broadcast({ type: "sprint-request", rid: pending, path, mine } satisfies SprintRequest).catch((e: Error) => setAnswer({ error: e.message }));
    };
    const off = wings.onBroadcast((message) => {
      const m = message as SprintResponse | null;
      if (m?.type === "sprint-response" && m.rid === pending) setAnswer(m);
    });
    const shown = ask;
    document.addEventListener("visibilitychange", shown);
    setAnswer(null);
    ask();
    const stop = pollWhileFocused(ask, POLL_MS);
    return () => {
      off();
      stop();
      document.removeEventListener("visibilitychange", shown);
    };
  }, [path, mine]);
  return answer;
}

/** Tells Wings how tall the content is, so the block fits it. */
function useFit(ref: React.RefObject<HTMLElement | null>) {
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = new ResizeObserver(() => void wings.setHeight(Math.ceil(el.getBoundingClientRect().height)));
    fit.observe(el);
    return () => fit.disconnect();
  }, [ref]);
}

export function Sprint() {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  const [state, setState] = useState<SprintBlockState | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    void wings.project().then(setProject);
    void Promise.resolve(wings.blockState()).then((raw) => setState(cleanBlockState(raw)));
  }, []);
  useFit(root);

  const setMine = (mine: boolean) => {
    const next = { mine };
    setState(next);
    void wings.setBlockState(next);
  };

  let body;
  if (project === undefined || !state) body = <Loading>Loading sprint</Loading>;
  else if (!project) body = <Text tone="muted">No project.</Text>;
  else body = <ProjectSprint key={project.id} path={project.path} mine={state.mine} onMine={setMine} />;
  return <div ref={root}>{body}</div>;
}

function ProjectSprint({ path, mine, onMine }: { path: string; mine: boolean; onMine: (mine: boolean) => void }) {
  const answer = useSprint(path, mine);
  // Not an Azure project: nothing to filter, so no chips.
  if (answer?.none) return <Text tone="muted">Not an Azure DevOps project.</Text>;

  const filter = (
    <Chips
      label="Show"
      value={mine ? "mine" : "all"}
      onChange={(v) => onMine(v === "mine")}
      options={[{ value: "mine", label: "Mine" }, { value: "all", label: "Whole sprint" }]}
    />
  );
  let body;
  if (!answer) body = <Loading>Loading sprint</Loading>;
  else if (answer.error) body = <div role="alert"><Empty title="Couldn't load the sprint" description={answer.error} /></div>;
  else if (!answer.sprints?.length) body = <Empty title="No sprint running" />;
  else if (!answer.items?.length) body = <Empty title={mine ? "Nothing assigned to you" : "No work items"} description={answer.sprints.join(", ")} />;
  else {
    const columns = groupByState(answer.items);
    const more = (answer.total ?? 0) - answer.items.length;
    body = (
      <Stack gap="s">
        <Board className="sprint-board" label={`Work items in ${answer.sprints.join(", ")}`}>
          {columns.map((c) => (
            <BoardColumn key={c.state} title={c.state} count={c.items.length}>
              {c.items.map((item) => <ItemCard key={item.id} item={item} org={answer.org!} project={answer.project!} showAssignee={!mine} />)}
            </BoardColumn>
          ))}
        </Board>
        {more > 0 ? <Text tone="muted" size="tiny">{more} more not shown</Text> : null}
      </Stack>
    );
  }
  return (
    <Stack gap="s">
      <Row gap="s" justify="between">
        <Text tone="muted" size="small" truncate>{answer?.sprints?.join(", ") ?? ""}</Text>
        {filter}
      </Row>
      {body}
    </Stack>
  );
}

function ItemCard({ item, org, project, showAssignee }: { item: BlockItem; org: string; project: string; showAssignee: boolean }) {
  return (
    <Card compact className="sprint-card">
      <Button
        variant="ghost"
        onClick={() => void wings.openView("item", { as: "tab", args: { id: item.id, org, project } })}
        aria-label={`Open ${item.type} ${item.id}: ${item.title}${showAssignee && item.assignee ? `, assigned to ${item.assignee}` : ""}`}
      >
        <Stack gap="xs">
          <Text weight="medium" clamp>{item.title}</Text>
          <Row gap="s">
            <TypeIcon type={item.type} size={14} />
            <Text tone="muted" size="tiny" numeric>#{item.id}</Text>
            {showAssignee && item.assignee ? <span className="sprint-assignee" title={item.assignee}><Avatar name={item.assignee} size="small" /></span> : null}
          </Row>
        </Stack>
      </Button>
    </Card>
  );
}

