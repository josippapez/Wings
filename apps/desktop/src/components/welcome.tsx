import { open } from "@tauri-apps/plugin-dialog";
import { FolderPlusIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Kbd } from "@/components/ui/kbd";

/** What to do first, shown when no project is open. Shortcuts are written for the platform and the saved global shortcut. */
export function Welcome({ paletteKey, jumpKey, hotkey, onAdd }: { paletteKey: string; jumpKey: string; hotkey: string | null; onAdd: (path: string) => void }) {
  async function choose() {
    const path = await open({ directory: true, multiple: false, title: "Add a project to Wings" });
    if (typeof path === "string") onAdd(path);
  }
  const steps: { text: string; keys?: string }[] = [
    { text: "Add a project: pick a folder, and Wings opens a terminal in it." },
    { text: "Run claude or opencode in a pane. Wings shows the agent's state in the sidebar and on its tab." },
    { text: "Press the command palette key to find any command, project, tab or past session. It lists every shortcut.", keys: paletteKey },
    { text: "Jump to the agent that needs you.", keys: jumpKey },
  ];
  return (
    <Empty className="h-full">
      <EmptyHeader className="max-w-md">
        <EmptyTitle>Welcome to Wings</EmptyTitle>
        <EmptyDescription>A terminal workspace for coding agents.</EmptyDescription>
      </EmptyHeader>
      <ol className="flex max-w-md list-none flex-col gap-2.5 text-left text-[13px] text-muted-foreground">
        {steps.map((step, i) => (
          <li key={i} className="flex items-start gap-3">
            <span className="mt-px flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-[11px] font-medium text-foreground" aria-hidden>
              {i + 1}
            </span>
            <span className="flex-1">{step.text}</span>
            {step.keys && <Kbd>{step.keys}</Kbd>}
          </li>
        ))}
      </ol>
      {hotkey && <p className="max-w-md text-[12px] text-muted-foreground">From any app, <Kbd>{hotkey}</Kbd> brings Wings forward and goes to that agent. Change it in Settings, Keyboard.</p>}
      <Button onClick={() => void choose()}>
        <FolderPlusIcon data-icon="inline-start" />
        Add a project
      </Button>
    </Empty>
  );
}
