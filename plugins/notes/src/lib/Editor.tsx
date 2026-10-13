// The notes editor for one project, shared by the sidebar and the tab. The text lives in the textarea, not in React
// state, so a keystroke costs no render and no storage write: it asks main to save 800 ms after typing stops, and on
// blur. Only main writes storage, so an agent's section added meanwhile is kept.

import { useEffect, useRef, useState } from "react";
import { Empty, Loading, Stack, Text } from "wings-plugin-ui";
import { MAX_CHARS, readNotes, type NotesMessage, type SaveMessage } from "./notes";

const SAVE_MS = 800;

type Status = "saved" | "unsaved" | "error";

export function Editor({ rows }: { rows: number }) {
  const [project, setProject] = useState<WingsProject | null | undefined>(undefined);
  useEffect(() => wings.onProject(setProject), []);
  if (project === undefined) return <Loading>Loading notes</Loading>;
  if (!project) return <Empty title="No project on screen" description="Open a project to see its notes." />;
  return <ProjectNotes key={project.id} projectId={project.id} name={project.name} rows={rows} />;
}

function ProjectNotes({ projectId, name, rows }: { projectId: string; name: string; rows: number }) {
  const area = useRef<HTMLTextAreaElement>(null);
  /** What storage held when this page last looked, to tell an agent's append from your own edit. */
  const stored = useRef("");
  /** The textarea's latest text, kept so the cleanup can still send it after the textarea is gone. */
  const latest = useRef("");
  const dirty = useRef(false);
  /** The save main hasn't answered yet: its id and the text it carried. */
  const inflight = useRef<{ rid: string; text: string } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const count = useRef(0);
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState<Status>("saved");

  const send = useRef(() => {});
  send.current = () => {
    clearTimeout(timer.current);
    if (!dirty.current) return;
    dirty.current = false;
    const request = { rid: String(++count.current), text: latest.current };
    inflight.current = request;
    void wings.broadcast({ type: "save", projectId, base: stored.current, ...request } satisfies SaveMessage).catch((error) => {
      console.error(error);
      dirty.current = true;
      setStatus("error");
    });
  };

  useEffect(() => {
    /** Takes what storage holds now. Sections added since stay at the end of what you're typing. */
    const adopt = (text: string) => {
      const el = area.current;
      if (!el || text === stored.current) return;
      if (!dirty.current) {
        el.value = latest.current = text;
      } else if (text.startsWith(stored.current)) {
        el.value += text.slice(stored.current.length);
        latest.current = el.value;
      }
      stored.current = text;
    };

    void readNotes(projectId).then(({ text }) => {
      stored.current = latest.current = text;
      if (area.current) area.current.value = text;
      setReady(true);
    });

    const off = wings.onBroadcast((message) => {
      const m = message as NotesMessage | null;
      if (m?.type !== "notes" || m.projectId !== projectId) return;
      const request = m.rid !== undefined && m.rid === inflight.current?.rid ? inflight.current : null;
      if (request) inflight.current = null;
      if (request && m.error) {
        dirty.current = true;
        setStatus("error");
        return;
      }
      void readNotes(projectId).then(({ text }) => {
        // Main saved what this page sent, with any sections added meanwhile after it.
        if (request) stored.current = request.text;
        adopt(text);
        if (request && !dirty.current) setStatus("saved");
      });
    });

    const flush = () => send.current();
    window.addEventListener("pagehide", flush);
    return () => {
      off();
      window.removeEventListener("pagehide", flush);
      flush();
    };
  }, [projectId]);

  return (
    <Stack gap="s">
      <textarea
        ref={area}
        aria-label={`Notes for ${name}`}
        placeholder="Write what Claude should know about this project. Markdown is fine."
        rows={rows}
        maxLength={MAX_CHARS}
        disabled={!ready}
        spellCheck
        onInput={(e) => {
          latest.current = e.currentTarget.value;
          dirty.current = true;
          clearTimeout(timer.current);
          timer.current = setTimeout(() => send.current(), SAVE_MS);
          if (status !== "unsaved") setStatus("unsaved");
        }}
        onBlur={() => send.current()}
      />
      <Text tone={status === "error" ? "danger" : "muted"} role="status">
        {status === "saved" ? `Saved for ${name}` : status === "unsaved" ? "Saving..." : `Couldn't save. Your text is still here. Notes are limited to ${MAX_CHARS.toLocaleString("en")} characters.`}
      </Text>
    </Stack>
  );
}
