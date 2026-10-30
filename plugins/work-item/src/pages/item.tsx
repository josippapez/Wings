// The work item tab: loads the work item and its parts, and hands them to ItemView (ui/item-view.tsx), which draws
// them. The page shows at once and each part fills in as its call returns.

import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { Empty, Page, Toolbar, useAction, useViewArgs } from "wings-plugin-ui";

import { az, azRun, invoke, workItem } from "../lib/az";
import { allLinkIds, commentsOf, discussionOf, idsQuery, itemArgs, itemTabArgs, linkIds, linkedItems, pullRequestLinks, revisionsOf, type ItemArgs, type LinkedItem } from "../lib/item";
import { ch } from "../lib/messages";
import { prUrl, workItemUrl, type WorkItemUpdate } from "../lib/urls";
import type { QueryRow } from "../lib/sections";
import type { TypeState } from "../lib/view";
import { ItemError, ItemSkeleton, ItemView, type ItemViewProps, type Part, type PrInfo, type Shown } from "../ui/item-view";
import "./item.css";

export const page = { place: "tab", fill: true };

const none = { data: null, error: null };

function Item({ args }: { args: ItemArgs }) {
  const { org, id } = args;
  const [item, setItem] = useState<Shown | null>(null);
  const [states, setStates] = useState<TypeState[]>([]);
  const [linked, setLinked] = useState<Part<Record<number, LinkedItem>>>(none);
  const [updates, setUpdates] = useState<ItemViewProps["updates"]>(none);
  const [prs, setPrs] = useState<PrInfo[]>([]);
  const [movingTo, setMovingTo] = useState("");
  // The error of loading the work item is the page's error card, and the error of a move shows over the work item.
  const { run: runLoad, error } = useAction();
  const move = useAction();
  const open = useAction();

  const load = useCallback(
    () =>
      runLoad(async () => {
        const found = await workItem<Shown["fields"]>(org, id);
        if (!found) throw new Error(`Work item ${id} doesn't exist, or you can't see it.`);
        const shown: Shown = { id: found.id, fields: found.fields, relations: found.relations ?? null };
        setItem(shown);
        const f = shown.fields;
        const project = f["System.TeamProject"];
        // The rest is independent: each part shows when it arrives, and one failing doesn't blank the others.
        void az<{ value: TypeState[] }>(org, invoke("wit", "workItemTypeStates", { project, type: f["System.WorkItemType"] }), { empty: { value: [] } })
          .then((r) => setStates(r.value))
          .catch(() => setStates([]));
        const ids = allLinkIds(linkIds(shown.relations, f["System.Parent"]));
        if (ids.length) {
          // One query for every linked work item, not a call each.
          void az<QueryRow[]>(org, ["boards", "query", "--project", project, "--wiql", idsQuery(ids)], { empty: [] })
            .then((rows) => setLinked({ data: linkedItems(rows), error: null }))
            .catch((e) => setLinked({ data: null, error: errorMessage(e) }));
        } else {
          setLinked({ data: {}, error: null });
        }
        void az<{ value: WorkItemUpdate[] }>(org, invoke("wit", "updates", { project, id }), { empty: { value: [] } })
          .then(({ value: list }) => {
            const comments = commentsOf(list);
            const revisions = revisionsOf(list);
            setUpdates({ data: { comments, revisions, feed: discussionOf(comments, revisions) }, error: null });
          })
          .catch((e) => setUpdates({ data: null, error: errorMessage(e) }));
        const links = pullRequestLinks(shown.relations);
        setPrs(links);
        // Titles for the first ten, one call each; the rest show by number, behind Show all.
        void Promise.all(
          links.slice(0, 10).map(async (link): Promise<PrInfo> => {
            try {
              const pr = await az<{ title?: string; status?: string; repository?: { name?: string } }>(org, invoke("git", "pullRequests", { project, pullRequestId: link.id }));
              return { ...link, title: pr.title, status: pr.status, repo: pr.repository?.name };
            } catch {
              return link;
            }
          }),
        ).then((first) => setPrs([...first, ...links.slice(10)]));
      }),
    [runLoad, org, id],
  );

  useEffect(() => void load(), [load]);
  useEffect(() => ch.on("item-changed", () => void load()), [load]);

  if (error) return <ItemError id={id} message={error} onRetry={() => void load()} />;
  if (!item) return <ItemSkeleton id={id} />;

  const project = item.fields["System.TeamProject"];
  const onMove = (to: string) => {
    setMovingTo(to);
    void move.run(async () => {
      await azRun(org, ["boards", "work-item", "update", "--id", String(id), "--state", to]);
      await load();
      // The pane badge in main shows the new state.
      void ch.tell("item-changed", { org, id });
    });
  };

  return (
    <ItemView
      args={args}
      item={item}
      states={states}
      linked={linked}
      updates={updates}
      prs={prs}
      moving={move.busy ? movingTo : null}
      moveError={move.error ?? open.error}
      onMove={onMove}
      onRefresh={() => void load()}
      onOpenItem={(target) => void wings.openView("item", { as: "tab", args: itemTabArgs(target, org) })}
      onOpenUrl={(url) => void open.run(() => wings.openUrl(url))}
      prUrl={(p) => prUrl(org, project, p.repoId, p.id)}
      workItemUrl={workItemUrl(org, project, id)}
    />
  );
}

export default function Root() {
  const args = useViewArgs(itemArgs);
  if (!args) return <Page toolbar={<Toolbar title="Work item" />}><Empty title="No work item" description="Open a work item from a pane's badge or a project's Work items." /></Page>;
  return <Item args={args} />;
}
