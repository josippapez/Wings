import { useState } from "react";
import { Button, Row, Text, TextInput } from "wings-plugin-ui";
import { LIMITS, type Op } from "./tasks";

/** A line to type a title in. Enter or Add sends it to main; the input clears when main accepts it. */
export function AddTask({ apply, branch }: { apply: (op: Op) => Promise<void>; branch?: string | null }) {
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!title.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await apply({ kind: "add", title, branch: branch ?? undefined });
      setTitle("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Row as="form" gap="s" onSubmit={(e) => { e.preventDefault(); void submit(); }}>
      <TextInput label="New task" placeholder="Add a task" value={title} maxLength={LIMITS.title} onChange={(e) => setTitle(e.currentTarget.value)} />
      <Button type="submit" size="sm" busy={busy} disabled={!title.trim()}>Add</Button>
      {error ? <Text tone="danger" role="alert">{error}</Text> : null}
    </Row>
  );
}
