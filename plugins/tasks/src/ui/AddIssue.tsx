import { useState } from "react";
import { Button, Row, Text, TextInput } from "wings-plugin-ui";
import { LIMITS, type Op } from "../lib/domain";

/** A line to type a title in. Enter or Add sends it to main; the input clears when main accepts it. */
export function AddIssue({ apply }: { apply: (op: Op) => Promise<void> }) {
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!title.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await apply({ kind: "create", title });
      setTitle("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Row as="form" gap="s" className="tk-add" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
      <TextInput label="New issue" placeholder="Add an issue" value={title} maxLength={LIMITS.title} onChange={(e) => setTitle(e.currentTarget.value)} />
      <Button type="submit" busy={busy} disabled={!title.trim()}>Add</Button>
      {error ? <Text tone="danger" role="alert" size="small">{error}</Text> : null}
    </Row>
  );
}
