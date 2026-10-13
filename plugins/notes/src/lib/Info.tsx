// The project info page: facts as a table, then the sections rendered as Markdown, each editable in place. Used by the tab.
// Typing costs no render and no storage write: a section's text lives in its textarea, and main is asked to save 800 ms
// after typing stops, and on blur. Only main writes storage.

import { memo, useEffect, useRef, useState } from "react";
import { Button, Card, Empty, Heading, Loading, Markdown, Row, Stack, Text, TextInput } from "wings-plugin-ui";
import "./info.css";
import { request, useInfo } from "./client";
import { baseOf, LIMITS, sizeOf, type Fact, type Section } from "./model";

const SAVE_MS = 800;

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function InfoPage({ projectId, name }: { projectId: string; name: string }) {
  const { info, error: unreadable } = useInfo(projectId);
  const [scrolled, setScrolled] = useState(false);

  // A click in the sidebar opens the tab with the section to show.
  useEffect(() => {
    const target = wings.viewArgs<{ section?: string }>()?.section;
    if (!info || scrolled) return;
    setScrolled(true);
    if (target) requestAnimationFrame(() => document.getElementById(`section-${target}`)?.scrollIntoView({ block: "start" }));
  }, [info, scrolled]);

  if (unreadable) return <Empty title={`The info for ${name} can't be read`} description={unreadable} />;
  if (info === undefined) return <Loading>Loading project info</Loading>;
  const { facts, sections } = info ?? { facts: [], sections: [] };
  return (
    <Stack gap="l" className="info">
      <Stack gap="s">
        <Heading>Facts</Heading>
        <FactsTable projectId={projectId} facts={facts} />
      </Stack>
      <Stack gap="m">
        <Heading>Sections</Heading>
        {sections.length === 0 && <Empty title={`Nothing written about ${name} yet`} description="Add a section below, or ask Claude to keep what it learns here." />}
        {sections.map((section, i) => (
          <SectionCard key={section.id} projectId={projectId} section={section} first={i === 0} last={i === sections.length - 1} />
        ))}
        <AddSection projectId={projectId} full={sections.length >= LIMITS.sections} />
      </Stack>
      <Text tone="muted" size="small">
        {sizeOf(info ?? { v: 1, facts, sections, updatedAt: 0 }).toLocaleString("en")} of {LIMITS.total.toLocaleString("en")} characters used. Images are https or small pasted data links.
      </Text>
    </Stack>
  );
}

// ---------- facts ----------

function FactsTable({ projectId, facts }: { projectId: string; facts: Fact[] }) {
  const [editing, setEditing] = useState<string | null>(null);
  const [error, setError] = useState("");
  const run = (job: Promise<void>) => job.then(() => setError(""), (e) => setError(message(e)));
  return (
    <Stack gap="s">
      {facts.length > 0 && (
        <div className="info-table-wrap">
          <table className="info-facts">
            <tbody>
              {facts.map((f) =>
                editing === f.key ? (
                  <FactForm
                    key={f.key}
                    initial={f}
                    onCancel={() => setEditing(null)}
                    onSave={(key, value) => run(request(projectId, { kind: "setFact", key, value, oldKey: f.key }).then(() => setEditing(null)))}
                  />
                ) : (
                  <tr key={f.key}>
                    <th scope="row">{f.key}</th>
                    <td>
                      <Markdown inline source={f.value} />
                    </td>
                    <td className="info-actions">
                      <Button size="xs" variant="ghost" onClick={() => setEditing(f.key)} aria-label={`Edit ${f.key}`}>
                        Edit
                      </Button>
                      <Button size="xs" variant="ghost" onClick={() => void run(request(projectId, { kind: "removeFact", key: f.key }))} aria-label={`Remove ${f.key}`}>
                        Remove
                      </Button>
                    </td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
      )}
      {facts.length < LIMITS.facts && <FactForm add onSave={(key, value) => run(request(projectId, { kind: "setFact", key, value }))} />}
      {error && (
        <Text tone="danger" role="alert">
          {error}
        </Text>
      )}
    </Stack>
  );
}

/** A row of two inputs: edits a fact, or with `add` shows an empty one under the table. */
function FactForm({ initial, add, onSave, onCancel }: { initial?: Fact; add?: boolean; onSave: (key: string, value: string) => Promise<void>; onCancel?: () => void }) {
  const key = useRef<HTMLInputElement>(null);
  const value = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!key.current || !value.current || !key.current.value.trim()) return;
    setBusy(true);
    try {
      await onSave(key.current.value, value.current.value);
      if (add) {
        key.current.value = value.current.value = "";
        key.current.focus();
      }
    } finally {
      setBusy(false);
    }
  };
  const fields = (
    <>
      <TextInput ref={key} label="Fact name" placeholder="Dev server" maxLength={LIMITS.key} defaultValue={initial?.key} onKeyDown={(e) => e.key === "Enter" && void submit()} />
      <TextInput ref={value} label="Fact value" placeholder="pnpm dev, port 5173" maxLength={LIMITS.value} defaultValue={initial?.value} onKeyDown={(e) => e.key === "Enter" && void submit()} />
      <Button size="sm" variant="secondary" busy={busy} onClick={() => void submit()}>
        {add ? "Add fact" : "Save"}
      </Button>
      {onCancel && (
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      )}
    </>
  );
  return add ? (
    <Row gap="s" wrap className="info-fact-form">
      {fields}
    </Row>
  ) : (
    <tr>
      <td colSpan={3}>
        <Row gap="s" wrap className="info-fact-form">
          {fields}
        </Row>
      </td>
    </tr>
  );
}

// ---------- sections ----------

const same = (a: Section, b: Section) => a.id === b.id && a.title === b.title && a.body === b.body && a.by === b.by && a.updatedAt === b.updatedAt;

/** Re-renders only when its own section or position changes, so typing in one doesn't render the others. */
const SectionCard = memo(
  function SectionCard({ projectId, section, first, last }: { projectId: string; section: Section; first: boolean; last: boolean }) {
    const [editing, setEditing] = useState(false);
    const [confirm, setConfirm] = useState(false);
    const [error, setError] = useState("");
    const act = (op: Parameters<typeof request>[1]) => request(projectId, op).then(() => setError(""), (e) => setError(message(e)));
    return (
      <Card id={`section-${section.id}`} className="info-section">
        {editing ? (
          <SectionEditor projectId={projectId} section={section} onDone={() => setEditing(false)} />
        ) : (
          <>
            <Heading level={3} actions={
              <Row gap="xs">
                <Button size="xs" variant="ghost" disabled={first} aria-label={`Move ${section.title} up`} onClick={() => void act({ kind: "moveSection", id: section.id, dir: -1 })}>Up</Button>
                <Button size="xs" variant="ghost" disabled={last} aria-label={`Move ${section.title} down`} onClick={() => void act({ kind: "moveSection", id: section.id, dir: 1 })}>Down</Button>
                <Button size="xs" variant="outline" onClick={() => setEditing(true)} aria-label={`Edit ${section.title}`}>Edit</Button>
                {confirm ? (
                  <>
                    <Button size="xs" variant="destructive" onClick={() => void act({ kind: "deleteSection", id: section.id })}>Delete section</Button>
                    <Button size="xs" variant="ghost" onClick={() => setConfirm(false)}>Keep</Button>
                  </>
                ) : (
                  <Button size="xs" variant="ghost" onClick={() => setConfirm(true)} aria-label={`Delete ${section.title}`}>Delete</Button>
                )}
              </Row>
            }>
              {section.title}
            </Heading>
            {section.body.trim() ? <Markdown source={section.body} /> : <Text tone="muted">Empty. Press Edit to write.</Text>}
            <Text tone="muted" size="tiny">
              {section.by === "agent" ? "Written by an agent" : "Written by you"}, {new Date(section.updatedAt).toLocaleString()}
            </Text>
          </>
        )}
        {error && (
          <Text tone="danger" role="alert">
            {error}
          </Text>
        )}
      </Card>
    );
  },
  (a, b) => a.projectId === b.projectId && a.first === b.first && a.last === b.last && same(a.section, b.section),
);

function SectionEditor({ projectId, section, onDone }: { projectId: string; section: Section; onDone: () => void }) {
  const area = useRef<HTMLTextAreaElement>(null);
  /** The section's text as main last confirmed it, to tell an agent's append from your own typing. */
  const base = useRef(section.body);
  const latestBody = useRef(section.body);
  const latestTitle = useRef(section.title);
  const dirty = useRef(false);
  const inflight = useRef(false);
  /** Whether this editor ever sent a save. After that, the section prop can lag behind what it saved, so only an extension of it is taken. */
  const sent = useRef(false);
  /** The section prop when the last save went out: the value it shows until it catches up with that save. */
  const stale = useRef("");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [status, setStatus] = useState<"saved" | "unsaved" | "error">("saved");
  const [error, setError] = useState("");
  /** Counts finished saves, so a section that changed while one was out is looked at again. */
  const [settled, setSettled] = useState(0);

  // Takes what storage holds. An agent's append goes after what you typed; an older body (the prop catching up with a
  // save this editor made) is ignored. Not while a save is out: its answer brings the rest.
  useEffect(() => {
    const el = area.current;
    const body = section.body;
    if (!el || inflight.current || body === base.current || (sent.current && body === stale.current)) return;
    const extension = body.startsWith(base.current);
    if (sent.current && !extension) return;
    if (!dirty.current) {
      el.value = latestBody.current = body;
    } else if (extension) {
      el.value += body.slice(base.current.length);
      latestBody.current = el.value;
    } else return;
    base.current = body;
  }, [section.body, settled]);

  const sectionRef = useRef(section);
  sectionRef.current = section;
  const send = useRef(() => {});
  send.current = () => {
    clearTimeout(timer.current);
    // One save at a time: a second would be merged against a base the first already changed.
    if (!dirty.current || inflight.current) return;
    dirty.current = false;
    inflight.current = true;
    sent.current = true;
    stale.current = sectionRef.current.body;
    const body = latestBody.current;
    request(projectId, { kind: "editSection", id: section.id, title: latestTitle.current, body, base: baseOf(base.current) }).then(
      () => {
        base.current = body;
        inflight.current = false;
        if (dirty.current) send.current();
        else setStatus("saved");
        setSettled((n) => n + 1);
      },
      (e) => {
        inflight.current = false;
        dirty.current = true;
        // A save that got no answer may still have landed: start the next one from what storage holds now.
        if (e instanceof Error && e.name === "Timeout") base.current = sectionRef.current.body;
        setError(message(e));
        setStatus("error");
      },
    );
  };

  useEffect(() => {
    const flush = () => send.current();
    window.addEventListener("pagehide", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      flush();
    };
  }, []);

  const touched = () => {
    dirty.current = true;
    clearTimeout(timer.current);
    timer.current = setTimeout(() => send.current(), SAVE_MS);
    setStatus((s) => (s === "unsaved" ? s : "unsaved"));
  };

  return (
    <Stack gap="s">
      <Row gap="s">
        <TextInput
          label="Section title"
          defaultValue={section.title}
          maxLength={LIMITS.title}
          onInput={(e) => {
            latestTitle.current = e.currentTarget.value;
            touched();
          }}
          onBlur={() => send.current()}
        />
        <Button size="sm" variant="secondary" onClick={() => { send.current(); onDone(); }}>
          Done
        </Button>
      </Row>
      <textarea
        ref={area}
        className="info-area"
        aria-label={`${section.title}, Markdown`}
        placeholder="Markdown: lists, tables, [links](https://example.com), ![images](https://example.com/a.png)"
        rows={12}
        maxLength={LIMITS.body}
        defaultValue={section.body}
        spellCheck
        autoFocus
        onInput={(e) => {
          latestBody.current = e.currentTarget.value;
          touched();
        }}
        onBlur={() => send.current()}
      />
      <Text tone={status === "error" ? "danger" : "muted"} size="small" role="status">
        {status === "saved" ? "Saved" : status === "unsaved" ? "Saving..." : `Couldn't save: ${error} Your text is still here.`}
      </Text>
    </Stack>
  );
}

function AddSection({ projectId, full }: { projectId: string; full: boolean }) {
  const title = useRef<HTMLInputElement>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  if (full) return <Text tone="muted">A project is limited to {LIMITS.sections} sections.</Text>;
  const add = async () => {
    if (!title.current?.value.trim()) return;
    setBusy(true);
    try {
      await request(projectId, { kind: "addSection", title: title.current.value });
      title.current.value = "";
      setError("");
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Stack gap="s">
      <Row gap="s" wrap>
        <TextInput ref={title} label="New section title" placeholder="Architecture, How to release, Gotchas" maxLength={LIMITS.title} onKeyDown={(e) => e.key === "Enter" && void add()} />
        <Button size="sm" variant="secondary" busy={busy} onClick={() => void add()}>
          Add section
        </Button>
      </Row>
      {error && (
        <Text tone="danger" role="alert">
          {error}
        </Text>
      )}
    </Stack>
  );
}
