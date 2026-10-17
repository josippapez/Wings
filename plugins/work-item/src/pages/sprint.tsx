// The Sprint block of a Project page: the work items of the sprint running now in the project's Azure DevOps
// project, a column per state. The main script runs the queries and keeps them fresh (main/sprint.ts); this page shows them,
// and a click opens the work item in a tab.

import { truncate } from "wings-plugin-sdk/format";
import { useQuery } from "wings-plugin-sdk/react";
import { Avatar, Board, BoardColumn, Button, Card, Chips, Empty, Loading, ProjectGate, Row, Stack, Text, useBlockState } from "wings-plugin-ui";
import { itemTabArgs } from "../lib/item";
import { ch } from "../lib/messages";
import { cleanBlockState, groupByState, type BlockItem } from "../lib/sections";
import { TypeIcon } from "../ui/TypeIcon";
import "./sprint.css";

export const page = { place: "block" };

export default function Sprint() {
  const [state, setState] = useBlockState({ mine: true }, cleanBlockState);
  return (
    <ProjectGate loading="Loading sprint">
      {(project) => (state ? <ProjectSprint path={project.path} mine={state.mine} onMine={(mine) => setState({ mine })} /> : <Loading>Loading sprint</Loading>)}
    </ProjectGate>
  );
}

function ProjectSprint({ path, mine, onMine }: { path: string; mine: boolean; onMine: (mine: boolean) => void }) {
  const { data, error } = useQuery(ch, "sprint", { path, mine });
  // Not an Azure project: nothing to filter, so no chips.
  if (data && "none" in data) return <Text tone="muted">Not an Azure DevOps project.</Text>;

  const filter = (
    <Chips
      label="Show"
      value={mine ? "mine" : "all"}
      onChange={(v) => onMine(v === "mine")}
      options={[{ value: "mine", label: "Mine" }, { value: "all", label: "Whole sprint" }]}
    />
  );
  let body;
  if (error) body = <div role="alert"><Empty title="Couldn't load the sprint" description={truncate(error.message, 300)} /></div>;
  else if (!data) body = <Loading>Loading sprint</Loading>;
  else if (!data.sprints.length) body = <Empty title="No sprint running" />;
  else if (!data.items.length) body = <Empty title={mine ? "Nothing assigned to you" : "No work items"} description={data.sprints.join(", ")} />;
  else {
    const columns = groupByState(data.items);
    const more = data.total - data.items.length;
    body = (
      <Stack gap="s">
        <Board className="sprint-board" label={`Work items in ${data.sprints.join(", ")}`}>
          {columns.map((c) => (
            <BoardColumn key={c.state} title={c.state} count={c.items.length}>
              {c.items.map((item) => <ItemCard key={item.id} item={item} org={data.org} showAssignee={!mine} />)}
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
        <Text tone="muted" size="small" truncate>{data && !error ? data.sprints.join(", ") : ""}</Text>
        {filter}
      </Row>
      {body}
    </Stack>
  );
}

function ItemCard({ item, org, showAssignee }: { item: BlockItem; org: string; showAssignee: boolean }) {
  return (
    <Card compact className="sprint-card">
      <Button
        variant="ghost"
        onClick={() => void wings.openView("item", { as: "tab", args: itemTabArgs(item.id, org) })}
        aria-label={`Open ${item.type} ${item.id}: ${item.title}${showAssignee && item.assignee ? `, assigned to ${item.assignee}` : ""}`}
      >
        <Stack gap="xs">
          <Text weight="medium" clamp title={item.title}>{item.title}</Text>
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
