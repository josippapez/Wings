// The Issues page in Wings Settings: which view the Issues tab opens in.

import { useEffect, useState } from "react";
import { Heading, Select, Stack, Text } from "wings-plugin-ui";
import { DEFAULT_VIEW_KEY, DEFAULT_VIEWS, defaultViewOf, type DefaultView } from "./view-setting";

export function IssuesSettings() {
  const [view, setView] = useState<DefaultView | null>(null);
  useEffect(() => {
    void wings.storage.get(DEFAULT_VIEW_KEY).then((saved) => setView(defaultViewOf(saved)));
  }, []);
  const change = (value: string) => {
    const next = defaultViewOf(value);
    setView(next);
    void wings.storage.set(DEFAULT_VIEW_KEY, next);
  };
  return (
    <Stack gap="s">
      <Heading>Issues board</Heading>
      <Text tone="muted">The view the Issues tab opens in. You can still switch views in the tab, until you close it.</Text>
      {view === null ? null : <Select label="Default view" options={DEFAULT_VIEWS} value={view} onChange={change} />}
    </Stack>
  );
}
