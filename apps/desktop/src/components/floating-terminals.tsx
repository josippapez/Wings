import { useEffect, useRef, useState } from "react";
import { MinusIcon, PanelBottomIcon, SquareTerminalIcon, XIcon } from "lucide-react";

import { FloatingPanel, PANEL_EDGE, type Rect } from "@/components/floating-panel";
import { IconButton } from "@/components/icon-button";
import type { PaneLabelInfo } from "@/components/pane-label";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { terminals } from "@/lib/terminal";
import { cn } from "@/lib/utils";

type PanelState = { rect: Rect; collapsed: boolean; active: string | null };

/** The panel of each agent pane, for as long as Wings runs: a restructured tree remounts the pane that shows it. */
const states = new Map<string, PanelState>();
const initial = (): PanelState => ({ rect: { right: PANEL_EDGE, bottom: PANEL_EDGE, w: 520, h: 300 }, collapsed: false, active: null });

/** One floated terminal, here and in the quick terminal. Its element moves in from the split it left and refits at the panel's size once that holds still. */
export function Host({ paneKey, shown, visible, keepRenderer, initialInput }: { paneKey: string; shown: boolean; visible: boolean; keepRenderer: boolean; initialInput: string | null }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const term = terminals.get(paneKey);
    if (term && host.current) term.attach(host.current, initialInput);
  }, [paneKey, initialInput]);
  useEffect(() => {
    terminals.get(paneKey)?.setVisible(shown && visible, keepRenderer);
  }, [paneKey, shown, visible, keepRenderer]);
  return <div ref={host} className={cn("min-h-0 flex-1 pr-1 pb-0.5 pl-3", !shown && "hidden")} />;
}

/**
 * The terminals an agent opened, floated over its pane instead of split below it: drag the header to move the panel,
 * the corner to resize it, minimise it to a bar to glance at later, or close the terminal it shows. Moves and resizes touch only the panel's own
 * style, so the agent's terminal never reflows during a gesture, and the floated terminal refits once at the end.
 */
export function FloatingTerminals({ ownerKey, ownerTitle, commands, labels, visible, keepRenderer, initialInputs, onDock, onClose }: {
  ownerKey: string;
  ownerTitle: string;
  commands: string[];
  labels: Record<string, PaneLabelInfo>;
  visible: boolean;
  keepRenderer: boolean;
  initialInputs: Record<string, string | null>;
  onDock: () => void;
  onClose: (paneKey: string) => void;
}) {
  const [state, setState] = useState<PanelState>(() => states.get(ownerKey) ?? initial());
  const keep = (next: PanelState) => {
    states.set(ownerKey, next);
    setState(next);
  };
  const active = commands.includes(state.active ?? "") ? (state.active as string) : commands[commands.length - 1];
  const label = labels[active];
  const { rect, collapsed } = state;

  const names = (key: string) => labels[key]?.title ?? "Terminal";
  const bodies = commands.map((key) => <Host key={key} paneKey={key} shown={!collapsed && key === active} visible={visible} keepRenderer={keepRenderer} initialInput={initialInputs[key] ?? null} />);

  return (
    <FloatingPanel
      rect={rect}
      collapsed={collapsed}
      onRect={(next) => keep({ ...state, rect: next })}
      onExpand={() => keep({ ...state, collapsed: false })}
      label={`Terminals opened by ${ownerTitle}`}
      header={
        <>
          <SquareTerminalIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
          {commands.length > 1 && !collapsed ? (
            <Tabs value={active} onValueChange={(key) => keep({ ...state, active: String(key) })} className="min-w-0 gap-0">
              <TabsList className="h-auto min-w-0 gap-1 rounded-none bg-transparent p-0">
                {commands.map((key) => (
                  <TabsTrigger
                    key={key}
                    value={key}
                    data-no-drag
                    className="h-auto max-w-28 flex-none justify-start px-1.5 py-0.5 text-[12px] font-normal text-muted-foreground after:hidden data-active:bg-hover data-active:shadow-none data-active:font-medium data-active:text-foreground dark:data-active:border-transparent dark:data-active:bg-hover"
                  >
                    <span className="truncate">{names(key)}</span>
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
          ) : (
            <span className="min-w-0 truncate font-medium">{names(active)}</span>
          )}
          <span className="min-w-0 truncate text-muted-foreground">
            {collapsed && commands.length > 1 ? `+${commands.length - 1} · ` : ""}
            {label?.detail ? `${label.detail} · ` : ""}by {ownerTitle}
          </span>
          <span className="flex-1" />
          {collapsed ? (
            <>
              <IconButton label="Dock back into the split" data-no-drag className="size-5" onClick={onDock}>
                <PanelBottomIcon />
              </IconButton>
              {/* A click on the bar opens the panel: the others would pop open as this one closes. */}
              <IconButton
                label={`Close ${names(active)}`}
                data-no-drag
                className="size-5"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(active);
                }}
              >
                <XIcon />
              </IconButton>
            </>
          ) : (
            <>
              <IconButton label="Minimise to a bar" data-no-drag onClick={() => keep({ ...state, collapsed: true })}>
                <MinusIcon />
              </IconButton>
              <IconButton label="Dock back into the split" data-no-drag onClick={onDock}>
                <PanelBottomIcon />
              </IconButton>
              <IconButton label={`Close ${names(active)}`} data-no-drag onClick={() => onClose(active)}>
                <XIcon />
              </IconButton>
            </>
          )}
        </>
      }
    >
      {/* The terminals stay mounted while minimised, hidden, so scrollback and the shell are untouched. */}
      {bodies}
    </FloatingPanel>
  );
}
