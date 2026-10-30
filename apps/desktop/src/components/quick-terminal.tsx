import { useEffect, useState } from "react";
import { MinusIcon, SquareTerminalIcon, XIcon } from "lucide-react";

import { FloatingPanel, PANEL_EDGE, type Rect } from "@/components/floating-panel";
import { Host } from "@/components/floating-terminals";
import { IconButton } from "@/components/icon-button";
import { tildify } from "@/components/pane-label";
import { QUICK_TERMINAL_KEY, type QuickState } from "@/lib/quick-terminal";
import { terminals } from "@/lib/terminal";
import { cn } from "@/lib/utils";

/** Where the panel was left, for as long as Wings runs. The shell isn't kept, only the place. */
let remembered: Rect = { right: PANEL_EDGE * 2, bottom: PANEL_EDGE * 2, w: 640, h: 380 };

/**
 * A terminal in the home folder that belongs to no project, floated over the pane area and toggled by a shortcut.
 * Hidden, it keeps its shell. It's the same panel as the terminals an agent opens: drag the header, resize from the
 * corner, minimise to a bar.
 */
export function QuickTerminal({ state, cwd, shortcut, onMinimise, onExpand, onClose }: {
  state: QuickState;
  /** Where the shell is now, when Wings tracks it. */
  cwd: string | null;
  shortcut: string | null;
  onMinimise: () => void;
  onExpand: () => void;
  onClose: () => void;
}) {
  const [rect, setRect] = useState(remembered);
  const shown = state.phase === "shown";
  const title = cwd ? tildify(cwd) : "~";

  // Showing it moves the cursor into the shell. Its terminal opens in Host's effect, which runs before this one.
  useEffect(() => {
    if (shown && !state.minimised) terminals.get(QUICK_TERMINAL_KEY)?.focus();
  }, [shown, state.minimised]);

  return (
    <FloatingPanel
      rect={rect}
      collapsed={state.minimised}
      onRect={(next) => setRect((remembered = next))}
      onExpand={onExpand}
      label="Quick terminal"
      overlayClassName={cn("z-30", !shown && "hidden")}
      header={
        <>
          <SquareTerminalIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
          <span className="min-w-0 truncate font-medium">{title}</span>
          <span className="min-w-0 truncate text-muted-foreground">Quick terminal{shortcut ? ` · ${shortcut}` : ""}</span>
          <span className="flex-1" />
          {!state.minimised && (
            <IconButton label="Minimise to a bar" data-no-drag onClick={onMinimise}>
              <MinusIcon />
            </IconButton>
          )}
          <IconButton
            label="Close and end the shell"
            data-no-drag
            className={state.minimised ? "size-5" : undefined}
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
      <Host paneKey={QUICK_TERMINAL_KEY} shown={!state.minimised} visible={shown} keepRenderer initialInput={null} />
    </FloatingPanel>
  );
}
