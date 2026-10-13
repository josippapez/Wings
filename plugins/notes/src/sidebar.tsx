// The Project info sidebar: the focused project's facts and section titles. Click a section to open it in the tab.

import { Button, Empty, Heading, ListButton, ListItem, List, Loading, Markdown, Properties, showSidebar, Stack, Text } from "wings-plugin-ui";
import { useInfo, useProject } from "./lib/client";

const openTab = (section?: string) => void wings.openView("editor", { as: "tab", args: section ? { section } : undefined }).catch(console.error);

function Compact({ projectId, name }: { projectId: string; name: string }) {
  const { info, error } = useInfo(projectId);
  if (error) return <Empty title={`The info for ${name} can't be read`} description={error} />;
  if (info === undefined) return <Loading>Loading</Loading>;
  if (!info || (!info.facts.length && !info.sections.length)) {
    return <Empty title={`No info for ${name} yet`} description="Open the tab to add facts and sections, or ask Claude to keep what it learns here." />;
  }
  return (
    <Stack gap="m">
      {info.facts.length > 0 && <Properties items={info.facts.map((f) => ({ label: f.key, value: <Markdown inline source={f.value} /> }))} />}
      {info.sections.length > 0 && (
        <Stack gap="xs">
          <Text tone="muted" size="small">Sections</Text>
          <List compact>
            {info.sections.map((s) => (
              <ListItem key={s.id}>
                <ListButton onClick={() => openTab(s.id)}>{s.title}</ListButton>
              </ListItem>
            ))}
          </List>
        </Stack>
      )}
    </Stack>
  );
}

function Sidebar() {
  const project = useProject();
  return (
    <Stack gap="m">
      <Heading actions={<Button size="xs" variant="outline" onClick={() => openTab()}>Open in tab</Button>}>Project info</Heading>
      {project === undefined ? <Loading>Loading</Loading> : project ? <Compact key={project.id} projectId={project.id} name={project.name} /> : <Empty title="No project on screen" description="Open a project to see its info." />}
    </Stack>
  );
}

showSidebar(<Sidebar />);
