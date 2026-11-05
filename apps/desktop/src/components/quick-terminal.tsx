import { useEffect, useState } from "react";
import { Maximize2Icon, Minimize2Icon, MinusIcon, SquareTerminalIcon, XIcon } from "lucide-react";

import { FloatingPanel, PANEL_EDGE, type Rect } from "@/components/floating-panel";
import { Host } from "@/components/floating-terminals";
import { IconButton } from "@/components/icon-button";
import { terminals } from "@/lib/terminal";
import { cn } from "@/lib/utils";

/** Where each panel was left, for as long as Wings runs. The shell isn't kept, only the place. */
const remembered = new Map<string, Rect>();
const initialRect = (): Rect => ({ right: PANEL_EDGE * 2, bottom: PANEL_EDGE * 2, w: 640, h: 380 });

/**
 * A terminal from the sidebar, in the home folder and in no project. `full` fills the pane area in place of the tab;
 * otherwise it floats over it like the terminals an agent opens: drag the header, resize from the corner, minimise to
 * a bar. Out of sight, it keeps its shell.
 */
export function QuickTerminal({ paneKey, title, shown, full, minimised, shortcut, onMinimise, onExpand, onFill, onFloat, onClose }: {
  paneKey: string;
  title: string;
  shown: boolean;
  full: boolean;
  minimised: boolean;
  shortcut: string | null;
  onMinimise: () => void;
  onExpand: () => void;
  onFill: () => void;
  onFloat: () => void;
  onClose: () => void;
}) {
  const [rect, setRect] = useState(() => remembered.get(paneKey) ?? initialRect());

  // Showing it moves the cursor into the shell. Its terminal opens in Host's effect, which runs before this one.
  useEffect(() => {
    if (shown && !minimised) terminals.get(paneKey)?.focus();
  }, [paneKey, shown, minimised, full]);

  return (
    <FloatingPanel
      rect={rect}
      collapsed={minimised && !full}
      fill={full}
      onRect={(next) => {
        remembered.set(paneKey, next);
        setRect(next);
      }}
      onExpand={onExpand}
      label="Terminal"
      overlayClassName={cn("z-30", !shown && "hidden")}
      header={
        <>
          <SquareTerminalIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
          <span className="min-w-0 truncate font-medium">{title}</span>
          {!full && shortcut && <span className="min-w-0 truncate text-muted-foreground">{shortcut}</span>}
          <span className="flex-1" />
          {(!minimised || full) && (
            <IconButton label={full ? "Detach to a floating panel" : "Fill the window like a tab"} data-no-drag onClick={full ? onFloat : onFill}>
              {full ? <Minimize2Icon /> : <Maximize2Icon />}
            </IconButton>
          )}
          {!minimised && !full && (
            <IconButton label="Minimise to a bar" data-no-drag onClick={onMinimise}>
              <MinusIcon />
            </IconButton>
          )}
          <IconButton
            label="Close and end the shell"
            data-no-drag
            className={minimised && !full ? "size-5" : undefined}
            onClick={(e) => {
              e.stopPropagation();
              onClose();
            }}
          >
            <XIcon />
          </IconButton>
        </>
      }
    >
      <Host paneKey={paneKey} shown={full || !minimised} visible={shown} keepRenderer initialInput={null} />
    </FloatingPanel>
  );
}
