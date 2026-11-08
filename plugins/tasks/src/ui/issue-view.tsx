// The Issue tab without its data: one issue with its spec, status, links, sessions and activity. The tab page feeds it
// from storage, a fixture from made-up data.

import { Fragment, useState, type ComponentProps, type ReactElement } from "react";
import { Button, Card, Empty, InlineInput, Loading, Menu, Page, Stack, Text, Title, Toolbar } from "wings-plugin-ui";
import { lastSession } from "../lib/actor";
import { AGENT_KINDS, AGENTS } from "../lib/commands";
import { canParent, LIMITS, type AgentKind, type FileChange, type Issue, type SessionRef } from "../lib/domain";
import { Activity, Children, Description, FilesChanged } from "./issue-body";
import { Properties } from "./issue-properties";
import { DeleteConfirm, ErrorText, TypeIcon, useScrollIntoView, type Apply, type Send } from "./parts";
import { Box, HeaderBox, Inline, Nav } from "./layout";

export interface IssueViewProps {
  project: WingsProject;
  /** `undefined` while it loads, `null` when the project has no issue with that key. */
  issue: Issue | null | undefined;
  /** Every issue of the project, for the parent, related and blocking pickers and the child list. */
  issues: Issue[];
  /** The key the tab was opened for, for the not-found page. */
  issueKey: string;
  error: string | null;
  /** Changes made from a part that stays open when it fails: the editors, and the delete. They show the reason themselves. */
  apply: Apply;
  send: Send;
  onBack: () => void;
  /** The issues opened before this one in this tab, oldest first, shown in the breadcrumb to go back to. */
  trail?: string[];
  /** Go back to `trail[index]`. */
  onBackTo?: (index: number) => void;
  /** Continues one of the issue's sessions in a new terminal, with the agent it ran in. */
  onResume: (session: SessionRef) => void;
  /** Opens the diff of one of the issue's changed files. */
  onOpenFile: (file: FileChange) => void;
  /** Starts an agent on the issue in a new terminal. */
  onStart: (kind: AgentKind) => void;
  onOpenUrl: (url: string) => void;
  /** Open another issue: `newTab` for a middle-click or Cmd/Ctrl+click. */
  onOpenIssue: (key: string, newTab: boolean) => void;
}

/** The native Text, which also takes `size="large"` (wings-plugin-ui/native.js passes it on). */
const NativeHeading = Text as unknown as (props: Omit<ComponentProps<typeof Text>, "size"> & { size: "large" }) => ReactElement;

/** The title, large, with the key beside it. Click it, or press Enter or F2 on it, to edit. */
function IssueTitle({ issue, apply }: { issue: Issue; apply: Apply }) {
  const [editing, setEditing] = useState(false);
  return (
    <Box className="tk-title-row">
      <TypeIcon type={issue.type} label />
      {editing ? (
        <InlineInput label="Title" className="tk-title-input" value={issue.title} maxLength={LIMITS.title} required onSave={(title) => apply({ kind: "update", key: issue.key, title })} onDone={() => setEditing(false)} />
      ) : WINGS_NATIVE ? (
        // The native app's buttons are one 14px line, so the title is large text that wraps, with its own Edit after
        // the key.
        <NativeHeading size="large" weight="strong" grow>{issue.title}</NativeHeading>
      ) : (
        <Title as="h1" className="tk-title">
          <Button variant="ghost" className="tk-title-button" title="Edit the title" onClick={() => setEditing(true)} onKeyDown={(e) => { if (e.key === "F2") setEditing(true); }}>
            {issue.title}
          </Button>
        </Title>
      )}
      <Text as="span" mono tone="muted" size="small" className="tk-title-key">{issue.key}</Text>
      {WINGS_NATIVE && !editing ? <Button variant="ghost" size="xs" aria-label="Edit the title" onClick={() => setEditing(true)}>Edit</Button> : null}
    </Box>
  );
}

export function IssueView({ project, issue, issues, issueKey, error, apply, send, onBack, trail = [], onBackTo, onResume, onOpenFile, onStart, onOpenUrl, onOpenIssue }: IssueViewProps) {
  const [confirming, setConfirming] = useState(false);
  const noticesRef = useScrollIntoView<HTMLDivElement>(confirming || error);
  const crumb = (
    <Nav className="tk-crumbs" aria-label="Breadcrumb">
      <Button variant="ghost" size="sm" onClick={onBack}><Inline aria-hidden="true">←</Inline> Issues</Button>
      {trail.map((key, i) => (
        <Fragment key={`${i}-${key}`}>
          <Inline aria-hidden="true" className="tk-crumb-sep tk-crumb-sep-link">/</Inline>
          <Button variant="ghost" size="sm" className="tk-crumb-link" onClick={() => onBackTo?.(i)}>{key}</Button>
        </Fragment>
      ))}
      <Inline aria-hidden="true" className="tk-crumb-sep">/</Inline>
      <Inline className="tk-crumb" aria-current="page">{issue?.key ?? issueKey}</Inline>
    </Nav>
  );
  const toolbar = (
    <Toolbar
      actions={
        issue ? (
          <>
            <Button variant="outline" size="sm" disabled={!lastSession(issue)} onClick={() => onResume(lastSession(issue)!)} title={lastSession(issue) ? "Open the latest session in a new terminal" : "No session to resume yet"}>Resume</Button>
            <Menu label="Start agent" size="sm" align="end" items={AGENT_KINDS.map((kind) => ({ id: kind, label: AGENTS[kind].label, onSelect: () => onStart(kind) }))} />
            <Menu label="…" aria-label="More" variant="ghost" size="sm" align="end" items={[{ id: "delete", label: "Delete issue", variant: "destructive", onSelect: () => setConfirming(true) }]} />
          </>
        ) : null
      }
    >
      {crumb}
    </Toolbar>
  );

  if (issue === undefined) return <Page toolbar={toolbar}><Loading>Loading issue</Loading></Page>;
  if (issue === null) {
    return (
      <Page toolbar={toolbar}>
        <Empty title={`No issue ${issueKey}`} description={`${project.name} has no issue with that key. It may have been removed.`}>
          <Button variant="outline" onClick={onBack}>Back to the board</Button>
        </Empty>
      </Page>
    );
  }

  return (
    <Page toolbar={toolbar} className="tk-page">
      <Box className="tk-wrap">
        {error || confirming ? (
          <Box ref={noticesRef}>
            <Stack gap="m" className="tk-notices">
              {error ? <Card><ErrorText>{error}</ErrorText></Card> : null}
              {confirming ? <DeleteConfirm issue={issue} issues={issues} onCancel={() => setConfirming(false)} onDelete={(children) => apply({ kind: "remove", key: issue.key, children })} /> : null}
            </Stack>
          </Box>
        ) : null}
        <Box className="tk-grid">
          <HeaderBox className="tk-head">
            <IssueTitle issue={issue} apply={apply} />
          </HeaderBox>
          <Properties issue={issue} issues={issues} project={project} apply={apply} send={send} onResume={onResume} onOpenUrl={onOpenUrl} onOpenIssue={onOpenIssue} />
          <Box className="tk-main">
            <Description issue={issue} apply={apply} />
            {canParent(issue.type) ? <Children issue={issue} issues={issues} onOpenIssue={onOpenIssue} /> : null}
            <FilesChanged issue={issue} onOpen={onOpenFile} />
            <Activity issue={issue} apply={apply} onOpenSession={onResume} />
          </Box>
        </Box>
      </Box>
    </Page>
  );
}
