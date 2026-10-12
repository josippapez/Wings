import { CodeIcon } from "lucide-react";

import { ContextMenuItem, ContextMenuSub, ContextMenuSubContent, ContextMenuSubTrigger } from "@/components/ui/context-menu";
import { defaultEditor, openInEditor, useDefaultEditorId, useEditors } from "@/lib/editors";

/** "Open in <editor>" for a project's context menu: the default editor, and with more installed a list to pick another. Nothing when none is installed. */
export function OpenInEditorItems({ spaceId }: { spaceId: string }) {
  const editors = useEditors();
  const [chosen] = useDefaultEditorId();
  const main = defaultEditor(editors, chosen);
  if (!main) return null;
  return (
    <>
      <ContextMenuItem onClick={() => openInEditor(main, spaceId)}>
        <CodeIcon />
        Open in {main.name}
      </ContextMenuItem>
      {editors.length > 1 && (
        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <CodeIcon />
            Open in…
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            {editors.map((e) => (
              <ContextMenuItem key={e.id} onClick={() => openInEditor(e, spaceId)}>
                {e.name}
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>
      )}
    </>
  );
}
