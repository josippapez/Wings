// The Work item page in Wings Settings: whether Azure DevOps updates are checked, and how often.

import { useEffect, useState } from "react";
import { Checkbox, Heading, Select, showSettings, Stack, Text } from "wings-plugin-ui";

import { DEFAULT_SETTINGS, type Settings } from "./lib/watch";

const INTERVALS = [2, 5, 10, 30].map((m) => ({ value: String(m), label: `Every ${m} minutes` }));

function WorkItemSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);

  useEffect(() => {
    void wings.storage.get("settings").then((saved) => setSettings({ ...DEFAULT_SETTINGS, ...(saved as Partial<Settings> | null) }));
  }, []);

  if (!settings) return null;
  const save = async (next: Settings) => {
    setSettings(next);
    await wings.storage.set("settings", next);
    wings.broadcast({ settingsChanged: true });
  };
  return (
    <Stack gap="m">
      <Heading>Azure DevOps updates</Heading>
      <Text tone="muted">Tells you when the state of a work item you own changes, when a comment on a work item or pull request mentions you, and about activity on pull requests you created or review. Read-only, and it keeps checking while Wings is in the background.</Text>
      <Checkbox checked={settings.enabled} onChange={(enabled) => save({ ...settings, enabled })}>
        Check for updates
      </Checkbox>
      <Select label="Check" disabled={!settings.enabled} value={String(settings.intervalMin)} onChange={(v) => v && save({ ...settings, intervalMin: Number(v) })} options={INTERVALS} />
    </Stack>
  );
}

showSettings(<WorkItemSettings />);
