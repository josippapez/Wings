// The work item tab: title, state with its next-state buttons, assignee, sprint, parent, the whole description,
// comments and linked pull requests. The page shows at once and each part fills in as its call returns.

import { useCallback, useEffect, useState } from "react";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Badge, Button, Card, Empty, Heading, Loading, Page, Properties, Row, showTab, Stack, Text, Toolbar } from "wings-plugin-ui";

import { Html } from "./lib/Html";
import { commentsOf, itemArgs, pullRequestLinks, type Comment, type ItemArgs, type PrLink } from "./lib/item";
import { prUrl, workItemUrl, type WorkItemUpdate } from "./lib/updates";
import { stateTone } from "./lib/sections";
import { nextStates, type TypeState } from "./lib/view";

interface Fields {
  "System.Title": string;
  "System.TeamProject": string;
  "System.WorkItemType": string;
  "System.State": string;
  "System.IterationPath"?: string;
  "System.Parent"?: number;
  "System.AssignedTo"?: { displayName?: string };
  "System.Description"?: string;
  "System.Tags"?: string;
}

interface Shown {
  id: number;
  fields: Fields;
  relations: { rel?: string; url?: string; attributes?: { name?: string } }[] | null;
}

interface PrInfo extends PrLink {
  title?: string;
  status?: string;
  repo?: string;
}

async function az(org: string, args: string[]): Promise<unknown> {
  const out = await wings.exec("az", [...args, "--organization", org, "-o", "json"]);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
  return out.stdout.trim() ? JSON.parse(out.stdout) : null;
}

const invoke = (area: string, resource: string, route: Record<string, string | number>) => [
  "devops", "invoke", "--area", area, "--resource", resource, "--api-version", "7.1",
  "--route-parameters", ...Object.entries(route).map(([k, v]) => `${k}=${v}`),
];

const when = (iso: string) => (iso ? new Date(iso).toLocaleString() : "");
const message = (e: unknown) => String((e as { message?: unknown })?.message ?? e);

function Item({ args }: { args: ItemArgs }) {
  const { org, id } = args;
  const [item, setItem] = useState<Shown | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [states, setStates] = useState<TypeState[]>([]);
  const [parent, setParent] = useState<string | null>(null);
  const [comments, setComments] = useState<Comment[] | null>(null);
  const [prs, setPrs] = useState<PrInfo[] | null>(null);
  const [moving, setMoving] = useState<string | null>(null);
  const [moveError, setMoveError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    let shown: Shown;
    try {
      shown = (await az(org, ["boards", "work-item", "show", "--id", String(id), "--expand", "relations"])) as Shown;
    } catch (e) {
      setError(message(e));
      return;
    }
    setItem(shown);
    const f = shown.fields;
    const project = f["System.TeamProject"];
    // The rest is independent: each part shows when it arrives, and one failing doesn't blank the others.
    void az(org, invoke("wit", "workItemTypeStates", { project, type: f["System.WorkItemType"] }))
      .then((r) => setStates((r as { value?: TypeState[] }).value ?? []))
      .catch(() => setStates([]));
    if (f["System.Parent"]) {
      void az(org, ["boards", "work-item", "show", "--id", String(f["System.Parent"])])
        .then((p) => setParent((p as Shown).fields["System.Title"] ?? ""))
        .catch(() => setParent(""));
    }
    void az(org, invoke("wit", "updates", { project, id }))
      .then((r) => setComments(commentsOf((r as { value?: WorkItemUpdate[] }).value ?? [])))
      .catch(() => setComments([]));
    const links = pullRequestLinks(shown.relations).slice(0, 10);
    setPrs(links);
    void Promise.all(
      links.map(async (link): Promise<PrInfo> => {
        try {
          const pr = (await az(org, invoke("git", "pullRequests", { project, pullRequestId: link.id }))) as { title?: string; status?: string; repository?: { name?: string } };
          return { ...link, title: pr.title, status: pr.status, repo: pr.repository?.name };
        } catch {
          return link;
        }
      }),
    ).then(setPrs);
  }, [org, id]);

  useEffect(() => void load(), [load]);
  useBroadcast((m) => {
    if ((m as { type?: string } | null)?.type === "item-changed") void load();
  });

  if (error) {
    return (
      <Page toolbar={<Toolbar title={`Work item #${id}`} actions={<Button size="sm" onClick={() => void load()}>Try again</Button>} />}>
        <Empty title="Couldn't load this work item" description={error} />
      </Page>
    );
  }
  if (!item) return <Page toolbar={<Toolbar title={`Work item #${id}`} />}><Loading /></Page>;

  const f = item.fields;
  const project = f["System.TeamProject"];
  const state = f["System.State"];
  const iteration = f["System.IterationPath"] ?? "";
  const next = nextStates({ state, states });
  const move = async (to: string) => {
    setMoving(to);
    setMoveError(null);
    try {
      const out = await wings.exec("az", ["boards", "work-item", "update", "--id", String(id), "--state", to, "--organization", org, "-o", "json"]);
      if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
      await load();
      // The pane badge in main shows the new state.
      void wings.broadcast({ type: "item-changed", id });
    } catch (e) {
      setMoveError(message(e));
    } finally {
      setMoving(null);
    }
  };
  const openItem = (target: number) => void wings.openView("item", { as: "tab", args: { id: target, org, project } });

  return (
    <Page
      toolbar={
        <Toolbar
          title={`#${id} ${f["System.Title"]}`}
          actions={
            <>
              <Button size="sm" variant="ghost" onClick={() => void load()}>Refresh</Button>
              <Button size="sm" onClick={() => void wings.openUrl(workItemUrl(org, project, id))}>Open in Azure DevOps</Button>
            </>
          }
        />
      }
    >
      <Stack gap="l">
        <Row gap="m" wrap>
          <Badge variant={stateTone(state) === "neutral" ? "outline" : undefined} tone={toneName(stateTone(state))}>{state}</Badge>
          <Text tone="muted">{f["System.WorkItemType"]}</Text>
          {next.map((s) => (
            <Button key={s} size="sm" busy={moving === s} onClick={() => void move(s)}>{`Move to ${s}`}</Button>
          ))}
        </Row>
        {moveError ? <Text tone="danger">{moveError}</Text> : null}
        <Properties
          items={[
            { label: "Assigned to", value: f["System.AssignedTo"]?.displayName ?? "Unassigned" },
            { label: "Sprint", value: iteration.includes("\\") ? iteration.split("\\").at(-1)! : "None" },
            ...(f["System.Tags"] ? [{ label: "Tags", value: f["System.Tags"] }] : []),
            ...(f["System.Parent"]
              ? [{ label: "Parent", value: <Button size="sm" variant="ghost" onClick={() => openItem(f["System.Parent"]!)}>{`#${f["System.Parent"]}${parent ? ` ${parent}` : ""}`}</Button> }]
              : []),
          ]}
        />

        <Stack gap="s">
          <Heading>Description</Heading>
          {f["System.Description"]?.trim() ? <Html html={f["System.Description"]} /> : <Text tone="muted">No description.</Text>}
        </Stack>

        <Stack gap="s">
          <Heading>Pull requests</Heading>
          {prs === null ? <Loading /> : prs.length === 0 ? <Text tone="muted">No linked pull requests.</Text> : (
            <Stack gap="s">
              {prs.map((p) => (
                <Row key={p.id} gap="s" justify="between">
                  <Text grow truncate>{`PR #${p.id}${p.title ? ` ${p.title}` : ""}`}</Text>
                  {p.status ? <Badge variant="outline">{p.status}</Badge> : null}
                  <Button size="sm" variant="ghost" onClick={() => void wings.openUrl(prUrl(org, project, p.repoId, p.id))}>Open</Button>
                </Row>
              ))}
            </Stack>
          )}
        </Stack>

        <Stack gap="s">
          <Heading>{comments ? `Comments (${comments.length})` : "Comments"}</Heading>
          {comments === null ? <Loading /> : comments.length === 0 ? <Text tone="muted">No comments.</Text> : (
            <Stack gap="m">
              {comments.map((c) => (
                <Card key={c.id} compact>
                  <Stack gap="xs">
                    <Row gap="s"><Text weight="strong">{c.author}</Text><Text tone="muted" size="small">{when(c.at)}</Text></Row>
                    <Html html={c.html} />
                  </Stack>
                </Card>
              ))}
            </Stack>
          )}
        </Stack>
      </Stack>
    </Page>
  );
}

const toneName = (t: string) => (t === "success" ? "success" : t === "warning" ? "warn" : t === "neutral" ? undefined : "info");

function Root() {
  const args = itemArgs(wings.viewArgs());
  if (!args) return <Page toolbar={<Toolbar title="Work item" />}><Empty title="No work item" description="Open a work item from a pane's badge, the Updates list or a project's Work items." /></Page>;
  return <Item args={args} />;
}

showTab(<Root />, { fill: true });
