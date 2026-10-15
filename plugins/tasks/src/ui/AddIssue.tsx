import { useState } from "react";
import { Button, Row, TextInput, useAction } from "wings-plugin-ui";
import { LIMITS, type Op } from "../lib/domain";
import { ErrorText } from "./parts";

/** A line to type a title in. Enter or Add sends it to main; the input clears when main accepts it. */
export function AddIssue({ apply }: { apply: (op: Op) => Promise<void> }) {
  const [title, setTitle] = useState("");
  const { run, busy, error } = useAction();

  const submit = () => {
    if (!title.trim() || busy) return;
    void run(async () => {
      await apply({ kind: "create", title });
      setTitle("");
    });
  };

  return (
    <Row as="form" gap="s" className="tk-add" onSubmit={(e) => { e.preventDefault(); submit(); }}>
      <TextInput label="New issue" placeholder="Add an issue" value={title} maxLength={LIMITS.title} onChange={(e) => setTitle(e.currentTarget.value)} />
      <Button type="submit" busy={busy} disabled={!title.trim()}>Add</Button>
      {error ? <ErrorText size="small">{error}</ErrorText> : null}
    </Row>
  );
}
