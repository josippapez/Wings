// The work item tab: loads the work item and its parts, and hands them to ItemView (ui/item-view.tsx), which draws
// them. The page shows at once and each part fills in as its call returns.

import { useCallback, useEffect, useState } from "react";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Empty, Page, Toolbar } from "wings-plugin-ui";

import { allLinkIds, commentsOf, discussionOf, idsQuery, itemArgs, linkIds, linkedItems, pullRequestLinks, revisionsOf, type ItemArgs, type LinkedItem } from "../lib/item";
import { prUrl, workItemUrl, type WorkItemUpdate } from "../lib/urls";
import type { QueryRow } from "../lib/sections";
import type { TypeState } from "../lib/view";
import { ItemError, ItemSkeleton, ItemView, type ItemViewProps, type Part, type PrInfo, type Shown } from "../ui/item-view";
import "./item.css";

export const page = { place: "tab", fill: true };

async function az(org: string, args: string[]): Promise<unknown> {
  const out = await wings.exec("az", [...args, "--organization", org, "-o", "json"]);
  if (out.code !== 0) throw new Error(out.stderr.trim().split("\n").at(-1) || "az failed");
  return out.stdout.trim() ? JSON.parse(out.stdout) : null;
}

const invoke = (area: string, resource: string, route: Record<string, string | number>) => [
  "devops", "invoke", "--area", area, "--resource", resource, "--api-version", "7.1",
  "--route-parameters", ...Object.entries(route).map(([k, v]) => `${k}=${v}`),
];

const message = (e: unknown) => String((e as { message?: unknown })?.message ?? e);
const none = { data: null, error: null };

function Item({ args }: { args: ItemArgs }) {
  const { org, id } = args;
  const [item, setItem] = useState<Shown | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [states, setStates] = useState<TypeState[]>([]);
  const [linked, setLinked] = useState<Part<Record<number, LinkedItem>>>(none);
  const [updates, setUpdates] = useState<ItemViewProps["updates"]>(none);
  const [prs, setPrs] = useState<PrInfo[]>([]);
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
    const ids = allLinkIds(linkIds(shown.relations, f["System.Parent"]));
    if (ids.length) {
      // One query for every linked work item, not a call each.
      void az(org, ["boards", "query", "--project", project, "--wiql", idsQuery(ids)])
        .then((rows) => setLinked({ data: linkedItems(Array.isArray(rows) ? (rows as QueryRow[]) : []), error: null }))
        .catch((e) => setLinked({ data: null, error: message(e) }));
    } else {
      setLinked({ data: {}, error: null });
    }
    void az(org, invoke("wit", "updates", { project, id }))
      .then((r) => {
        const list = (r as { value?: WorkItemUpdate[] }).value ?? [];
        const comments = commentsOf(list);
        const revisions = revisionsOf(list);
        setUpdates({ data: { comments, revisions, feed: discussionOf(comments, revisions) }, error: null });
      })
      .catch((e) => setUpdates({ data: null, error: message(e) }));
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

  if (error) return <ItemError id={id} message={error} onRetry={() => void load()} />;
  if (!item) return <ItemSkeleton id={id} />;

  const project = item.fields["System.TeamProject"];
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

  return (
    <ItemView
      args={args}
      item={item}
      states={states}
      linked={linked}
      updates={updates}
      prs={prs}
      moving={moving}
      moveError={moveError}
      onMove={(to) => void move(to)}
      onRefresh={() => void load()}
      onOpenItem={(target) => void wings.openView("item", { as: "tab", args: { id: target, org, project } })}
      onOpenUrl={(url) => void wings.openUrl(url)}
      prUrl={(p) => prUrl(org, project, p.repoId, p.id)}
      workItemUrl={workItemUrl(org, project, id)}
    />
  );
}

export default function Root() {
  const args = itemArgs(wings.viewArgs());
  if (!args) return <Page toolbar={<Toolbar title="Work item" />}><Empty title="No work item" description="Open a work item from a pane's badge or a project's Work items." /></Page>;
  return <Item args={args} />;
}
