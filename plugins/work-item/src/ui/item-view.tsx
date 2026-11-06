// The work item tab's page, drawn from what it's given: no loading and no talking to Azure in here, so the same
// components can be rendered with made-up data. item.tsx owns the loading and the requests.

import { useState, type ReactNode } from "react";
import { plural } from "wings-plugin-sdk/format";
import { Avatar, Badge, Button, Dot, Empty, Heading, Menu, Page, Skeleton, Skeletons, Summary, SummaryChip, Tabs, Text, Time, Timeline, TimelineCard, TimelineNote, Toolbar } from "wings-plugin-ui";

import { Html } from "./Html";
import { TypeIcon } from "./TypeIcon";
import { Article, Box, Em, H1, H3, HeaderBox, Inline, Item, LinkButton, Ordered, SectionBox, Strong, Unordered } from "./layout";
import { allLinkIds, lastSegment, linkIds, prStatus, summaryOf, tagsOf, type Activity, type Change, type Comment, type Fields, type ItemArgs, type LinkedItem, type PrLink, type Relation, type Revision } from "../lib/item";
import { stateTone } from "../lib/sections";
import { otherStates, type TypeState } from "../lib/view";

export interface Shown {
  id: number;
  fields: Fields;
  relations: Relation[] | null;
}

export interface PrInfo extends PrLink {
  title?: string;
  status?: string;
  repo?: string;
}

/** A part of the page that loads on its own: still on its way (both null), failed (`error`), or here. */
export interface Part<T> {
  data: T | null;
  error: string | null;
}

export interface ItemViewProps {
  args: ItemArgs;
  item: Shown;
  states: TypeState[];
  linked: Part<Record<number, LinkedItem>>;
  updates: Part<{ comments: Comment[]; revisions: Revision[]; feed: Activity[] }>;
  prs: PrInfo[];
  /** The state a move is in flight to. */
  moving: string | null;
  moveError: string | null;
  onMove: (to: string) => void;
  onRefresh: () => void;
  onOpenItem: (id: number) => void;
  onOpenUrl: (url: string) => void;
  /** The address of a pull request in the browser. */
  prUrl: (pr: PrInfo) => string;
  workItemUrl: string;
  /** The tab to open on, for a fixture; the page opens on the overview. */
  initialTab?: Tab;
}

type Tab = "overview" | "links" | "history";
type Tone = "info" | "success" | "warn" | undefined;

/** A work item state's tone, as the names plugin-ui's Badge and Dot take. */
const toneOf = (state: string): Tone => {
  const t = stateTone(state);
  return t === "success" ? "success" : t === "warning" ? "warn" : t === "neutral" ? undefined : "info";
};

const hours = (n: number) => `${Number.isInteger(n) ? n : n.toFixed(1)} h`;

function Toolbars({ id, type, onRefresh, url, onOpenUrl }: { id: number; type?: string; onRefresh?: () => void; url?: string; onOpenUrl?: (url: string) => void }) {
  return (
    <Toolbar
      title={`${type ?? "Work item"} #${id}`}
      actions={
        <>
          {onRefresh ? <Button variant="outline" size="sm" onClick={onRefresh}>Refresh</Button> : null}
          {url && onOpenUrl ? <Button size="sm" onClick={() => onOpenUrl(url)}>Open in Azure DevOps</Button> : null}
        </>
      }
    />
  );
}

/** The page before the work item arrives. */
export function ItemSkeleton({ id }: { id: number }) {
  return (
    <Page toolbar={<Toolbars id={id} />} className="wi-page" data-testid="wi-page">
      <Box className="wi-wrap">
        <Skeletons label="Loading work item" data-testid="wi-skeleton">
          <Skeleton width="30%" height={14} />
          <Skeleton width="80%" height={26} />
          <Skeleton width="55%" height={14} />
          <Skeleton height={90} />
          <Skeleton height={120} />
        </Skeletons>
      </Box>
    </Page>
  );
}

/** The work item couldn't be read. */
export function ItemError({ id, message, onRetry }: { id: number; message: string; onRetry: () => void }) {
  return (
    <Page toolbar={<Toolbars id={id} />} className="wi-page" data-testid="wi-page">
      <Box className="wi-wrap">
        <Box className="wi-card wi-error" role="alert" data-testid="wi-error">
          <Text weight="strong">Couldn't load this work item</Text>
          <Text tone="danger">{message}</Text>
          <Box><Button size="sm" variant="outline" onClick={onRetry}>Try again</Button></Box>
        </Box>
      </Box>
    </Page>
  );
}

export function ItemView(props: ItemViewProps) {
  const { item, args, linked, updates, prs, moveError, onRefresh, onOpenUrl, workItemUrl } = props;
  const f = item.fields;
  const [tab, setTab] = useState<Tab>(props.initialTab ?? "overview");
  const links = linkIds(item.relations, f["System.Parent"]);
  const linkCount = allLinkIds(links).length + prs.length;
  const comments = updates.data?.comments.length;
  const parent = links.parent ? linked.data?.[links.parent] : undefined;

  return (
    <Page toolbar={<Toolbars id={args.id} type={f["System.WorkItemType"]} onRefresh={onRefresh} url={workItemUrl} onOpenUrl={onOpenUrl} />} className="wi-page" data-testid="wi-page">
      <Box className="wi-wrap">
        <Header {...props} parentId={links.parent} parentTitle={parent?.title} />
        {moveError ? <Text tone="danger" role="alert">{moveError}</Text> : null}
        <SummaryRow item={item} prs={prs} childIds={links.children} linked={linked.data} onGo={() => setTab("links")} />
        <Tabs
          label="Work item"
          variant="line"
          value={tab}
          onChange={setTab}
          items={[
            { value: "overview", label: `Overview${comments ? ` ${comments}` : ""}` },
            { value: "links", label: `Links${linkCount ? ` ${linkCount}` : ""}` },
            { value: "history", label: `History${updates.data ? ` ${updates.data.revisions.length}` : ""}` },
          ]}
        >
          {tab === "overview" ? <OverviewTab item={item} updates={updates} onRefresh={onRefresh} /> : null}
          {tab === "links" ? <LinksTab {...props} links={links} /> : null}
          {tab === "history" ? <HistoryTab updates={updates} onRefresh={onRefresh} /> : null}
        </Tabs>
      </Box>
    </Page>
  );
}

// ---------- header ----------

function Header({ item, states, moving, onMove, onOpenItem, parentId, parentTitle }: ItemViewProps & { parentId: number | null; parentTitle?: string }) {
  const f = item.fields;
  const state = f["System.State"];
  const next = otherStates({ state, states });
  const tone = toneOf(state);
  const who = f["System.AssignedTo"]?.displayName;
  const sprint = lastSegment(f["System.IterationPath"]);
  const area = lastSegment(f["System.AreaPath"]);
  const tags = tagsOf(f["System.Tags"]);
  const created = f["System.CreatedDate"];
  const changed = f["System.ChangedDate"];
  const label = (
    <>
      <Dot tone={tone === "success" ? undefined : (tone ?? "muted")} />
      {moving ? `Moving to ${moving}…` : state}
    </>
  );

  return (
    <HeaderBox className="wi-header">
      <Box className="wi-kicker">
        <TypeIcon type={f["System.WorkItemType"]} />
        <Inline>{f["System.WorkItemType"]}</Inline>
        {parentId ? (
          <Button variant="link" size="xs" className="wi-parent" title="Open the parent work item" onClick={() => onOpenItem(parentId)} data-testid="wi-parent">
            {`Child of #${parentId}${parentTitle ? ` ${parentTitle}` : ""}`}
          </Button>
        ) : null}
      </Box>
      <H1 className="wi-title" data-testid="wi-title">
        {f["System.Title"]}
        <Inline className="wi-number">#{item.id}</Inline>
      </H1>
      <Box className="wi-meta">
        {next.length ? (
          <Menu
            variant="outline"
            size="sm"
            label={label}
            disabled={moving !== null}
            aria-label={`State ${state}. Move this work item`}
            items={next.map((s) => ({ id: s, label: `Move to ${s}`, onSelect: () => onMove(s) }))}
            data-testid="wi-state"
          />
        ) : (
          <Badge variant="outline" tone={tone} data-testid="wi-state">{label}</Badge>
        )}
        <Inline className="wi-person" data-testid="wi-assignee">
          {who ? <Avatar name={who} size="small" /> : null}
          {who ? <Strong>{who}</Strong> : <Text tone="muted" as="span">Unassigned</Text>}
        </Inline>
        {sprint ? <Badge variant="outline" title={`Sprint ${f["System.IterationPath"]}`}>{sprint}</Badge> : null}
        {area && area !== f["System.TeamProject"] ? <Badge variant="secondary" title={`Area ${f["System.AreaPath"]}`}>{area}</Badge> : null}
        {created ? <Text tone="muted" size="small" as="span"><Time iso={created} prefix={`created${f["System.CreatedBy"]?.displayName ? ` by ${f["System.CreatedBy"].displayName} ` : " "}`} /></Text> : null}
        {changed ? <Text tone="muted" size="small" as="span"><Time iso={changed} prefix="updated " /></Text> : null}
      </Box>
      {tags.length ? (
        <Unordered className="wi-tags" aria-label="Tags">
          {tags.map((t) => <Item key={t}><Badge variant="secondary">{t}</Badge></Item>)}
        </Unordered>
      ) : null}
    </HeaderBox>
  );
}

// ---------- summary ----------

function SummaryRow({ item, prs, childIds, linked, onGo }: { item: Shown; prs: PrInfo[]; childIds: number[]; linked: Record<number, LinkedItem> | null; onGo: () => void }) {
  const s = summaryOf(item.fields);
  const active = prs.filter((p) => p.status === "active").length;
  const done = linked ? childIds.filter((id) => linked[id] && stateTone(linked[id].state) === "success").length : null;
  const chips: ReactNode[] = [];
  if (s.priority !== undefined) chips.push(<SummaryChip key="p" tone={s.priority === 1 ? "danger" : s.priority === 2 ? "warn" : undefined} title="Priority">Priority {s.priority}</SummaryChip>);
  if (s.severity) chips.push(<SummaryChip key="sev" title="Severity">{s.severity} severity</SummaryChip>);
  if (s.estimate) chips.push(<SummaryChip key="e" title={s.estimate.label}>{s.estimate.value} {s.estimate.label.toLowerCase()}</SummaryChip>);
  if (s.remaining !== undefined) chips.push(<SummaryChip key="r" title="Remaining work">{hours(s.remaining)} left</SummaryChip>);
  if (prs.length) chips.push(<SummaryChip key="pr" tone={active ? "info" : prs.every((p) => p.status === "completed") ? "success" : undefined} onClick={onGo} data-testid="wi-prs">{plural(prs.length, "pull request")}{active ? ` · ${active} active` : ""}</SummaryChip>);
  else chips.push(<SummaryChip key="pr">No pull requests</SummaryChip>);
  if (childIds.length) chips.push(<SummaryChip key="c" onClick={onGo} data-testid="wi-children">{plural(childIds.length, "child", "children")}{done !== null ? ` · ${done} done` : ""}</SummaryChip>);
  return <Summary data-testid="wi-summary">{chips}</Summary>;
}

// ---------- overview ----------

function Loading({ label }: { label: string }) {
  return (
    <Skeletons label={label}>
      <Skeleton height={80} />
      <Skeleton height={80} />
    </Skeletons>
  );
}

function PartError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <Box className="wi-card wi-error" role="alert" data-testid="wi-part-error">
      <Text tone="danger">{message}</Text>
      <Box><Button size="sm" variant="outline" onClick={onRetry}>Try again</Button></Box>
    </Box>
  );
}

function Section({ title, html }: { title: string; html: string }) {
  return (
    <SectionBox className="wi-card" aria-label={title}>
      <H3 className="wi-card-title">{title}</H3>
      <Html html={html} />
    </SectionBox>
  );
}

function OverviewTab({ item, updates, onRefresh }: { item: Shown; updates: ItemViewProps["updates"]; onRefresh: () => void }) {
  const f = item.fields;
  const author = f["System.CreatedBy"]?.displayName;
  const description = f["System.Description"]?.trim();
  const repro = f["Microsoft.VSTS.TCM.ReproSteps"]?.trim();
  const acceptance = f["Microsoft.VSTS.Common.AcceptanceCriteria"]?.trim();
  const feed = updates.data?.feed;
  const comments = updates.data?.comments.length;

  return (
    <Box className="wi-overview">
      <Article className="wi-card" aria-label="Description" data-testid="wi-description">
        {author ? (
          <Box className="wi-card-head">
            <Avatar name={author} />
            <Strong>{author}</Strong>
            <Text tone="muted" size="small" as="span">created this {f["System.CreatedDate"] ? <Time iso={f["System.CreatedDate"]} /> : null}</Text>
          </Box>
        ) : null}
        {description ? <Html html={description} /> : <Text tone="muted">No description provided.</Text>}
      </Article>
      {repro ? <Section title="Repro steps" html={repro} /> : null}
      {acceptance ? <Section title="Acceptance criteria" html={acceptance} /> : null}

      <Heading level={3}>Discussion{comments !== undefined ? ` · ${comments}` : ""}</Heading>
      {updates.error ? (
        <PartError message={updates.error} onRetry={onRefresh} />
      ) : !feed ? (
        <Loading label="Loading discussion" />
      ) : !feed.some((a) => a.kind === "comment") && feed.length <= 1 ? (
        <Empty title="No comments yet" description="Comments and changes to the state, assignee and sprint show up here." />
      ) : (
        <Timeline label="Discussion" data-testid="wi-discussion">
          {feed.map((a) =>
            a.kind === "comment" ? (
              <TimelineCard key={a.key} author={a.author} at={a.at} clamp tag={a.author === author ? "Author" : undefined}>
                <Html html={a.html} />
              </TimelineCard>
            ) : (
              <TimelineNote key={a.key} author={a.author} at={a.at}>
                {a.created ? "created this work item" : describeAll(a.changes)}
              </TimelineNote>
            ),
          )}
        </Timeline>
      )}
    </Box>
  );
}

/** "changed state from New to Active", "set sprint to Sprint 5", "cleared assigned to". */
function describe(c: Change): ReactNode {
  const what = c.label.toLowerCase();
  if (c.label === "Assigned to") {
    if (c.to === undefined) return <>unassigned <Strong>{c.from}</Strong></>;
    return c.from === undefined ? <>assigned to <Strong>{c.to}</Strong></> : <>reassigned from <Strong>{c.from}</Strong> to <Strong>{c.to}</Strong></>;
  }
  if (c.to === undefined) return <>cleared {what} (was <Strong>{c.from}</Strong>)</>;
  if (c.from === undefined) return <>set {what} to <Strong>{c.to}</Strong></>;
  return <>changed {what} from <Strong>{c.from}</Strong> to <Strong>{c.to}</Strong></>;
}

function describeAll(changes: Change[]): ReactNode {
  return changes.map((c, i) => <Inline key={c.label}>{i ? ", " : ""}{describe(c)}</Inline>);
}

// ---------- links ----------

function LinkRow({ id, item, onOpen }: { id: number; item?: LinkedItem; onOpen: (id: number) => void }) {
  return (
    <Item>
      <LinkButton className="wi-link" onClick={() => onOpen(id)} aria-label={`Open #${id}${item ? ` ${item.title}` : ""}`} data-testid="wi-link">
        {item ? <TypeIcon type={item.type} /> : null}
        <Inline className="wi-link-id">#{id}</Inline>
        <Inline className="wi-link-title">{item?.title ?? ""}</Inline>
        {item?.state ? <Badge variant="outline" tone={toneOf(item.state)}>{item.state}</Badge> : null}
        {item?.assignee ? <Inline title={item.assignee}><Avatar name={item.assignee} size="small" /></Inline> : null}
      </LinkButton>
    </Item>
  );
}

/** The pull requests shown before "Show all". */
const PRS_SHOWN = 10;

function LinksTab({ links, linked, prs, onOpenItem, onOpenUrl, prUrl, onRefresh }: ItemViewProps & { links: ReturnType<typeof linkIds> }) {
  const [allPrs, setAllPrs] = useState(false);
  const groups: [string, number[]][] = [
    ["Parent", links.parent ? [links.parent] : []],
    ["Children", links.children],
    ["Related", links.related],
    ["Predecessors", links.predecessors],
    ["Successors", links.successors],
  ];
  const any = groups.some(([, ids]) => ids.length) || prs.length > 0;
  const waiting = groups.some(([, ids]) => ids.length) && !linked.data && !linked.error;

  if (!any) return <Box className="wi-overview"><Empty title="No links" description="Parent and child work items, related items and pull requests show up here." /></Box>;
  return (
    <Box className="wi-overview" data-testid="wi-links">
      {linked.error ? <PartError message={linked.error} onRetry={onRefresh} /> : null}
      {groups.filter(([, ids]) => ids.length).map(([title, ids]) => (
        <SectionBox key={title} className="wi-group" aria-label={title}>
          <Heading level={3}>{title} · {ids.length}</Heading>
          {waiting ? (
            <Skeletons label={`Loading ${title.toLowerCase()}`}>{ids.map((id) => <Skeleton key={id} height={36} />)}</Skeletons>
          ) : (
            <Unordered className="wi-links">{ids.map((id) => <LinkRow key={id} id={id} item={linked.data?.[id]} onOpen={onOpenItem} />)}</Unordered>
          )}
        </SectionBox>
      ))}
      {prs.length ? (
        <SectionBox className="wi-group" aria-label="Pull requests">
          <Heading level={3}>Pull requests · {prs.length}</Heading>
          <Unordered className="wi-links">
            {(allPrs ? prs : prs.slice(0, PRS_SHOWN)).map((p) => {
              const status = prStatus(p.status);
              return (
                <Item key={p.id}>
                  <LinkButton className="wi-link" onClick={() => onOpenUrl(prUrl(p))} aria-label={`Open pull request ${p.id}${p.title ? ` ${p.title}` : ""} in the browser`} data-testid="wi-pr">
                    <Inline className="wi-link-id">PR #{p.id}</Inline>
                    <Inline className="wi-link-title">{p.title ?? ""}</Inline>
                    {p.repo ? <Text tone="muted" size="small" as="span">{p.repo}</Text> : null}
                    {status.label ? <Badge variant="outline" tone={status.tone}>{status.label}</Badge> : null}
                  </LinkButton>
                </Item>
              );
            })}
          </Unordered>
          {prs.length > PRS_SHOWN && !allPrs ? <Button variant="link" size="sm" onClick={() => setAllPrs(true)}>Show all {prs.length}</Button> : null}
        </SectionBox>
      ) : null}
    </Box>
  );
}

// ---------- history ----------

function HistoryTab({ updates, onRefresh }: { updates: ItemViewProps["updates"]; onRefresh: () => void }) {
  if (updates.error) return <Box className="wi-overview"><PartError message={updates.error} onRetry={onRefresh} /></Box>;
  if (!updates.data) return <Box className="wi-overview"><Loading label="Loading history" /></Box>;
  const revisions = [...updates.data.revisions].reverse();
  if (!revisions.length) return <Box className="wi-overview"><Empty title="No history" description="State, assignee and sprint changes show up here." /></Box>;
  return (
    <Box className="wi-overview">
      <Ordered className="wi-history" aria-label="History" data-testid="wi-history">
        {revisions.map((r) => (
          <Item key={r.id} className="wi-rev">
            <Avatar name={r.by} size="small" />
            <Box className="wi-rev-body">
              <Box className="wi-rev-head">
                <Strong>{r.by}</Strong>
                <Text tone="muted" size="small" as="span">{r.created ? "created this work item" : null}</Text>
                <Time iso={r.at} className="wi-rev-time" />
              </Box>
              {r.changes.length ? (
                <Unordered className="wi-changes">
                  {r.changes.map((c) => (
                    <Item key={c.label} className="wi-change">
                      <Inline className="wi-change-label">{c.label}</Inline>
                      {c.from !== undefined ? <Inline className="wi-change-from">{c.from}</Inline> : null}
                      {c.from !== undefined && c.to !== undefined ? <Inline aria-label="to" className="wi-arrow">→</Inline> : null}
                      {c.to !== undefined ? <Strong>{c.to}</Strong> : <Em>cleared</Em>}
                    </Item>
                  ))}
                </Unordered>
              ) : null}
            </Box>
          </Item>
        ))}
      </Ordered>
    </Box>
  );
}
