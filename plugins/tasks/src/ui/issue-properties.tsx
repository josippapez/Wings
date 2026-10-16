// The properties column of the Issue page: status, priority and type, the links to other issues, the branch, the pull request,
// the commits and sessions, and when it was made and last changed.

import { useState } from "react";
import { Badge, Button, Combobox, copyText, InlineInput, Row, Select, Text, Time } from "wings-plugin-ui";
import { lastSession } from "../lib/actor";
import { LIMITS, PRIORITIES, PRIORITY_LABEL, STATUS_LABEL, STATUSES, TYPE_LABEL, TYPES, type Issue, type Priority, type Status } from "../lib/domain";
import { iso } from "../lib/format";
import { blocksOf, parentChoices } from "../lib/relations";
import { openProps, PriorityIcon, StatusDot, TypeIcon, type Apply, type Send } from "./parts";

const STATUS_OPTIONS = STATUSES.map((value) => ({ value, label: STATUS_LABEL[value] }));
const PRIORITY_OPTIONS = PRIORITIES.map((value) => ({ value, label: PRIORITY_LABEL[value] }));
const TYPE_OPTIONS = TYPES.map((value) => ({ value, label: TYPE_LABEL[value] }));

/** One property of the column: a small muted label over its value. */
function Prop({ label, wide, empty, actions, children }: { label: string; wide?: boolean; empty?: boolean; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="tk-prop" data-wide={wide ? "" : undefined} data-empty={empty ? "" : undefined}>
      <Row gap="s" justify="between" className="tk-prop-label">
        <Text as="span" tone="muted" size="tiny" weight="medium">{label}</Text>
        {actions}
      </Row>
      {children}
    </div>
  );
}

function Branch({ issue, project, apply, send }: { issue: Issue; project: WingsProject; apply: Apply; send: Send }) {
  const branch = issue.links?.branch ?? "";
  const [editing, setEditing] = useState(false);
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void copyText(branch).then(() => setCopied(true));
    setTimeout(() => setCopied(false), 1500);
  };
  const set = (value: string) => apply({ kind: "update", key: issue.key, branch: value });
  return (
    <Prop
      label="Branch"
      actions={
        editing ? null : (
          <Row gap="xs">
            {branch ? <Button size="xs" variant="ghost" aria-label="Copy the branch name" onClick={copy}>{copied ? "Copied" : "Copy"}</Button> : null}
            <Button size="xs" variant="ghost" aria-label="Edit the branch" onClick={() => setEditing(true)}>Edit</Button>
          </Row>
        )
      }
    >
      {editing ? (
        <InlineInput label="Branch" className="tk-prop-input" value={branch} maxLength={LIMITS.branch} placeholder="feature/name" onSave={set} onDone={() => setEditing(false)} />
      ) : branch ? (
        <Text as="span" mono size="small" className="tk-prop-mono" title={branch}>{branch}</Text>
      ) : (
        <Text as="span" tone="muted" size="small">None</Text>
      )}
      {!editing && project.branch && project.branch !== branch ? (
        <Button size="xs" variant="ghost" className="tk-prop-use" onClick={() => void send({ kind: "update", key: issue.key, branch: project.branch ?? "" })}>Use {project.branch}</Button>
      ) : null}
    </Prop>
  );
}

const prLabel = (url: string) => {
  const m = /\/(?:pull|pullrequest|pullrequests|merge_requests)\/(\d+)/.exec(url);
  if (m) return `PR #${m[1]}`;
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

function PullRequest({ issue, apply, onOpenUrl }: { issue: Issue; apply: Apply; onOpenUrl: (url: string) => void }) {
  const pr = issue.links?.pr ?? "";
  const [editing, setEditing] = useState(false);
  return (
    <Prop label="Pull request" actions={editing ? null : <Button size="xs" variant="ghost" aria-label="Edit the pull request link" onClick={() => setEditing(true)}>Edit</Button>}>
      {editing ? (
        <InlineInput label="Pull request URL" className="tk-prop-input" value={pr} maxLength={LIMITS.pr} placeholder="https://" onSave={(value) => apply({ kind: "update", key: issue.key, pr: value })} onDone={() => setEditing(false)} />
      ) : pr ? (
        <Button variant="outline" size="xs" className="tk-pr-chip" title={pr} onClick={() => onOpenUrl(pr)}>{prLabel(pr)} <span aria-hidden="true">↗</span></Button>
      ) : (
        <Text as="span" tone="muted" size="small">None</Text>
      )}
    </Prop>
  );
}

/** Another issue as a chip, "KEY title", that opens it. With `onRemove`, an × beside it takes the link away. */
function IssueChip({ issue, fallback, onOpen, onRemove }: { issue?: Issue; fallback: string; onOpen: (key: string, newTab: boolean) => void; onRemove?: () => void }) {
  return (
    <div className="tk-link">
      {issue ? (
        <Button variant="outline" size="xs" className="tk-link-chip" title={`${issue.key} ${issue.title}`} {...openProps((newTab) => onOpen(issue.key, newTab))}>
          <StatusDot status={issue.status} />
          <span className="tk-key">{issue.key}</span>
          <span className="tk-link-title">{issue.title}</span>
        </Button>
      ) : (
        <Text as="span" mono tone="muted" size="small">{fallback}</Text>
      )}
      {onRemove ? <Button size="icon-xs" variant="ghost" aria-label={`Remove ${fallback}`} onClick={onRemove}><span aria-hidden="true">×</span></Button> : null}
    </div>
  );
}

/** A field that finds an issue by key or title and hands back its key. */
function IssuePicker({ label, placeholder, options, onPick }: { label: string; placeholder: string; options: Issue[]; onPick: (key: string) => void }) {
  const [query, setQuery] = useState("");
  return (
    <Combobox
      freeText
      label={label}
      placeholder={placeholder}
      className="tk-prop-input"
      value={query}
      onInput={setQuery}
      options={options.map((o) => ({ value: o.key, label: o.key, detail: o.title }))}
      onPick={(o) => {
        setQuery("");
        onPick(o.value);
      }}
      empty="No issue matches"
    />
  );
}

function Links({ issue, issues, send, onOpenIssue }: { issue: Issue; issues: Issue[]; send: Send; onOpenIssue: (key: string, newTab: boolean) => void }) {
  const find = (key: string) => issues.find((i) => i.key === key);
  const others = issues.filter((i) => i.key !== issue.key);
  const blocks = blocksOf(issues, issue.key);
  const chips = (keys: string[] | undefined, remove: (key: string) => void) => (keys ?? []).map((k) => <IssueChip key={k} issue={find(k)} fallback={k} onOpen={onOpenIssue} onRemove={() => remove(k)} />);
  return (
    <>
      <Prop label="Parent" wide>
        {issue.parent ? <IssueChip issue={find(issue.parent)} fallback={issue.parent} onOpen={onOpenIssue} onRemove={() => void send({ kind: "update", key: issue.key, parent: "" })} /> : null}
        <IssuePicker label="Parent" placeholder={issue.parent ? "Change parent" : "Choose an epic or feature"} options={parentChoices(issues, issue).filter((p) => p.key !== issue.parent)} onPick={(parent) => void send({ kind: "update", key: issue.key, parent })} />
      </Prop>
      <Prop label="Related" wide empty={!issue.related?.length}>
        {chips(issue.related, (k) => void send({ kind: "update", key: issue.key, relatedRemove: [k] }))}
        <IssuePicker label="Related" placeholder="Add a related issue" options={others.filter((o) => !issue.related?.includes(o.key))} onPick={(k) => void send({ kind: "update", key: issue.key, relatedAdd: [k] })} />
      </Prop>
      <Prop label="Blocked by" wide empty={!issue.blockedBy?.length}>
        {chips(issue.blockedBy, (k) => void send({ kind: "update", key: issue.key, blockedByRemove: [k] }))}
        <IssuePicker label="Blocked by" placeholder="Add a blocking issue" options={others.filter((o) => !issue.blockedBy?.includes(o.key))} onPick={(k) => void send({ kind: "update", key: issue.key, blockedByAdd: [k] })} />
      </Prop>
      {blocks.length ? (
        <Prop label="Blocks" wide>
          {blocks.map((b) => <IssueChip key={b.key} issue={b} fallback={b.key} onOpen={onOpenIssue} />)}
        </Prop>
      ) : null}
    </>
  );
}

export function Properties({ issue, issues, project, apply, send, onOpenUrl, onOpenIssue }: { issue: Issue; issues: Issue[]; project: WingsProject; apply: Apply; send: Send; onOpenUrl: (url: string) => void; onOpenIssue: (key: string, newTab: boolean) => void }) {
  const last = lastSession(issue);
  const commits = issue.links?.commits ?? [];
  return (
    <aside className="tk-props" aria-label="Properties">
      <Prop label="Status">
        <Row gap="s" className="tk-status">
          <StatusDot status={issue.status} />
          <Select label="Status" className="tk-status-select" options={STATUS_OPTIONS} value={issue.status} onChange={(status: Status) => void send({ kind: "update", key: issue.key, status })} />
        </Row>
      </Prop>
      <Prop label="Priority">
        <Row gap="s" className="tk-status">
          <PriorityIcon priority={issue.priority} />
          <Select label="Priority" className="tk-status-select" options={PRIORITY_OPTIONS} value={issue.priority ?? "none"} onChange={(priority: Priority) => void send({ kind: "update", key: issue.key, priority })} />
        </Row>
      </Prop>
      <Prop label="Type">
        <Row gap="s" className="tk-status">
          <TypeIcon type={issue.type} />
          <Select label="Type" className="tk-status-select" options={TYPE_OPTIONS} value={issue.type} onChange={(type) => void send({ kind: "update", key: issue.key, type })} />
        </Row>
      </Prop>
      <Links issue={issue} issues={issues} send={send} onOpenIssue={onOpenIssue} />
      <Branch issue={issue} project={project} apply={apply} send={send} />
      <PullRequest issue={issue} apply={apply} onOpenUrl={onOpenUrl} />
      <Prop label="Commits" wide empty={!commits.length}>
        {commits.length ? (
          <Row gap="s" wrap>
            {commits.map((c) => <Badge key={c} variant="secondary" className="tk-sha">{c}</Badge>)}
          </Row>
        ) : (
          <Text as="span" tone="muted" size="small">None</Text>
        )}
      </Prop>
      <Prop label="Sessions" wide empty={!issue.sessions.length}>
        {issue.sessions.length === 0 ? (
          <Text as="span" tone="muted" size="small">None yet</Text>
        ) : (
          <ul className="tk-sessions" aria-label="Claude sessions">
            {[...issue.sessions].sort((a, b) => b.seen - a.seen).map((s) => (
              <li key={s.id} className="tk-session">
                <Row gap="s">
                  <Text as="span" size="small" truncate>{s.name || "Claude session"}</Text>
                  {s.id === last?.id ? <Badge variant="outline">Latest</Badge> : null}
                </Row>
                <Text as="span" tone="muted" size="tiny">
                  <span className="tk-sha" title={s.id}>{s.id.slice(0, 8)}</span> · <Time iso={iso(s.seen)} prefix="seen " />
                </Text>
              </li>
            ))}
          </ul>
        )}
      </Prop>
      <Prop label="Created">
        <Text as="span" size="small"><Time iso={iso(issue.createdAt)} /></Text>
      </Prop>
      <Prop label="Updated">
        <Text as="span" size="small"><Time iso={iso(issue.updatedAt)} /></Text>
      </Prop>
    </aside>
  );
}
