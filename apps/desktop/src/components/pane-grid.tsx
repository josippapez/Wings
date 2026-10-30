import { useEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { Columns2Icon, EyeOffIcon, Maximize2Icon, Minimize2Icon, PanelBottomIcon, PanelRightIcon, PencilIcon, PictureInPicture2Icon, Rows2Icon, SquarePlusIcon, SquareTerminalIcon, XIcon } from "lucide-react";

import { FindBar } from "@/components/find-bar";
import { FloatingTerminals } from "@/components/floating-terminals";
import { IconButton } from "@/components/icon-button";
import { InlineName } from "@/components/inline-name";
import { OverflowRow } from "@/components/overflow-row";
import { PaneBadge } from "@/components/pane-badge";
import { PaneRails, type PaneRailEntry } from "@/components/pane-rail";
import { PaneStatusStrip, type PaneStatusEntry } from "@/components/pane-status";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuSub, ContextMenuSubContent, ContextMenuSubTrigger, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { PaneIcon, type PaneLabelInfo } from "@/components/pane-label";
import { agentSplit, paneIds, paneNames, without, type LayoutNode } from "@/lib/layout";
import type { Badge } from "@/lib/plugins";
import { trackDrag } from "@/lib/pointer-drag";
import { shortcutLabel, terminals } from "@/lib/terminal";
import { cn } from "@/lib/utils";

export type PaneActions = {
  focus: (paneKey: string) => void;
  split: (paneKey: string, dir: "row" | "column") => void;
  close: (paneKey: string) => void;
  toggleZoom: (paneKey: string) => void;
  resize: (splitId: string, ratio: number) => void;
  /** An empty name goes back to the automatic one. */
  rename: (paneKey: string, name: string) => void;
  /** Moves the pane into another tab of the same project, or into a new one. */
  moveTo: (paneKey: string, tabId: string | "new") => void;
  /** Where the terminals an agent opened sit: split below its pane, split beside it, or floating over it. "split" brings a floated one back to the split it left. */
  agentView: (ownerKey: string, view: "below" | "right" | "float" | "split") => void;
};

type GridProps = {
  tabId: string;
  /** The project's other terminal tabs, for "Move to tab". */
  moveTargets: { id: string; title: string }[];
  layout: LayoutNode;
  visible: boolean;
  /** Hidden but recently shown: its terminals keep their WebGL renderers. */
  keepRenderer: boolean;
  focusedPane: string;
  zoomedPane: string | null;
  labels: Record<string, PaneLabelInfo>;
  initialInputs: Record<string, string | null>;
  badges: Record<string, { pluginId: string; badge: Badge }[]>;
  onBadgeAction: (pluginId: string, paneKey: string, actionId: string) => Promise<void>;
  /** What plugins show under each pane's terminal, and what a click on it opens. */
  statuses: Record<string, PaneStatusEntry[]>;
  onOpenStatus: (pluginId: string, page: string) => void;
  /** The cards plugins float over each pane's terminal. A click on a card's title opens its page, like a status. */
  rails: Record<string, PaneRailEntry[]>;
  /** Terminals an agent opened as splits below its own pane, by the agent's pane key: they hide and show together. */
  commands: Record<string, number>;
  /** The agent pane that opened each of those terminals, by the terminal's pane key. */
  openedBy: Record<string, string>;
  /** Agent panes whose terminals float over them instead of sitting in the split. */
  floated: ReadonlySet<string>;
  actions: PaneActions;
};

/**
 * One tab's panes. Every tab stays mounted (hidden when inactive) so terminals never lose their DOM; a restored
 * tab's shells start the first time it's shown.
 */
export function PaneGrid(props: GridProps) {
  // A floated terminal stays in the tree (so the layout keeps its place for it) but isn't a pane of the grid.
  const floating = new Set(Object.keys(props.openedBy).filter((key) => props.floated.has(props.openedBy[key]) && paneIds(props.layout).includes(props.openedBy[key])));
  const root = props.zoomedPane ? ({ kind: "pane", id: props.zoomedPane } as const) : floating.size ? without(props.layout, floating) : props.layout;
  return (
    <div
      data-pane-grid
      className={cn("absolute inset-0 flex p-2 pt-0", !props.visible && "invisible")}
      aria-hidden={!props.visible}
    >
      <Node node={root} {...props} />
    </div>
  );
}

function Node(props: GridProps & { node: LayoutNode }) {
  const { node } = props;
  if (node.kind === "pane") return <PaneCard {...props} paneKey={node.id} />;
  return <Split {...props} node={node} />;
}

function Split(props: GridProps & { node: Extract<LayoutNode, { kind: "split" }> }) {
  const { node } = props;
  const ref = useRef<HTMLDivElement>(null);
  const row = node.dir === "row";

  function startDrag(e: React.PointerEvent<HTMLDivElement>) {
    const box = ref.current?.getBoundingClientRect();
    if (!box) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const ratio = row ? (ev.clientX - box.left) / box.width : (ev.clientY - box.top) / box.height;
      props.actions.resize(node.id, Math.min(0.85, Math.max(0.15, ratio)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  return (
    <div ref={ref} className={cn("flex min-h-0 min-w-0 flex-1", row ? "flex-row" : "flex-col")}>
      <div className="flex min-h-0 min-w-0" style={{ flex: `0 0 calc(${node.ratio * 100}% - 4px)` }}>
        <Node {...props} node={node.a} />
      </div>
      <div
        role="separator"
        aria-orientation={row ? "vertical" : "horizontal"}
        onPointerDown={startDrag}
        className={cn(
          "group/divider relative shrink-0 touch-none",
          row ? "w-2 cursor-col-resize" : "h-2 cursor-row-resize",
        )}
      >
        <span
          className={cn(
            "absolute rounded-full transition-colors duration-150 group-hover/divider:bg-white/25 group-active/divider:bg-white/40",
            // The line between an agent and the terminals it opened shows at rest, so it reads as something to drag.
            paneIds(node.b).some((key) => props.openedBy[key]) ? "bg-white/10" : "bg-white/0",
            row ? "inset-y-6 left-[3px] w-0.5" : "inset-x-6 top-[3px] h-0.5",
          )}
        />
      </div>
      <div className="flex min-h-0 min-w-0 flex-1">
        <Node {...props} node={node.b} />
      </div>
    </div>
  );
}

function PaneCard(props: GridProps & { paneKey: string }) {
  const { paneKey, actions } = props;
  const host = useRef<HTMLDivElement>(null);
  const focused = props.focusedPane === paneKey;
  const zoomed = props.zoomedPane === paneKey;
  const label = props.labels[paneKey];
  const session = terminals.get(paneKey);
  const [editing, setEditing] = useState(false);
  /** A rename chosen in the context menu starts once the menu has closed, so closing doesn't take the focus back. */
  const renameAfterMenu = useRef(false);
  const named = paneKey in paneNames(props.layout);
  const others = props.moveTargets.filter((t) => t.id !== props.tabId);
  const owner = props.openedBy[paneKey];
  const ownerTitle = owner ? (props.labels[owner]?.title ?? "its agent") : "";
  const ownerSplit = owner ? agentSplit(props.layout, owner, new Set(Object.keys(props.openedBy))) : null;
  const floatedHere = props.floated.has(paneKey) ? Object.keys(props.openedBy).filter((key) => props.openedBy[key] === paneKey && paneIds(props.layout).includes(key)) : [];

  /**
   * Drag the pane by its header onto another tab, or onto + for a new one. The ghost and the drop highlight are set
   * straight on the DOM: a drag changes no React state, and nothing here resizes the pane area until the drop.
   */
  function dragPane(e: React.PointerEvent) {
    if (editing || (e.target as HTMLElement).closest("button,input,a,[role=separator]")) return;
    let ghost: HTMLElement | null = null;
    let over: HTMLElement | null = null;
    const target = (x: number, y: number) => {
      const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-pane-drop],[data-pane-drop-new]") ?? null;
      return el && el.dataset.tabId === props.tabId ? null : el;
    };
    trackDrag(e, {
      onStart: () => {
        ghost = document.createElement("div");
        ghost.textContent = label?.title ?? "Pane";
        ghost.className = "pointer-events-none fixed top-0 left-0 z-[100] max-w-56 truncate rounded-lg border border-hairline-strong bg-popover px-2.5 py-1 text-[12px] font-medium text-popover-foreground shadow-lg";
        document.body.append(ghost);
        document.body.style.cursor = "grabbing";
      },
      onMove: ({ x, y }) => {
        if (ghost) ghost.style.transform = `translate(${x + 12}px, ${y + 12}px)`;
        const next = target(x, y);
        if (next === over) return;
        over?.removeAttribute("data-drop-over");
        next?.setAttribute("data-drop-over", "");
        over = next;
      },
      onEnd: ({ x, y }, cancelled) => {
        const drop = cancelled ? null : target(x, y);
        over?.removeAttribute("data-drop-over");
        ghost?.remove();
        document.body.style.cursor = "";
        if (drop) actions.moveTo(paneKey, drop.dataset.tabId ?? "new");
      },
    });
  }

  // A split restructures the tree and remounts this card, so the terminal re-attaches to the new host here.
  useEffect(() => {
    const term = terminals.get(paneKey);
    if (term && host.current) term.attach(host.current, props.initialInputs[paneKey] ?? null);
  }, [paneKey, props.initialInputs]);

  useEffect(() => {
    terminals.get(paneKey)?.setVisible(props.visible, props.keepRenderer);
  }, [paneKey, props.visible, props.keepRenderer]);

  useEffect(() => {
    if (props.visible && focused) terminals.get(paneKey)?.focus();
  }, [paneKey, props.visible, focused]);

  return (
    <motion.section
      initial={{ opacity: 0, scale: 0.985 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
      onMouseDown={() => actions.focus(paneKey)}
      data-pane-key={paneKey}
      aria-label={label?.title}
      className={cn(
        "group/pane flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-[14px] border bg-surface transition-[border-color,box-shadow] duration-200",
        focused
          ? "border-hairline-strong shadow-[0_10px_30px_rgb(0_0_0/0.28),inset_0_1px_0_rgb(255_255_255/0.05)]"
          : "border-hairline",
      )}
    >
      <ContextMenu
        onOpenChangeComplete={(open) => {
          if (open || !renameAfterMenu.current) return;
          renameAfterMenu.current = false;
          setEditing(true);
        }}
      >
      <ContextMenuTrigger
        render={<div onPointerDown={(e) => dragPane(e)} />}
        className="flex h-7.5 shrink-0 cursor-grab items-center gap-2 pr-1 pl-3.5 active:cursor-grabbing"
      >
        <PaneIcon agent={label?.agent} />
        {editing ? (
          <InlineName
            initial={label?.title ?? ""}
            label="Pane name"
            className="h-6 w-40 text-[13px] font-semibold"
            onDone={(name) => {
              setEditing(false);
              if (name !== null) actions.rename(paneKey, name);
            }}
          />
        ) : (
          <span onDoubleClick={() => setEditing(true)} className={cn("max-w-[50%] truncate text-[13px] font-semibold", !focused && "text-muted-foreground")}>
            {label?.title}
          </span>
        )}
        {label?.detail && <span className="min-w-0 truncate text-[12px] text-muted-foreground">{label.detail}</span>}
        {owner && (
          <span title={`This terminal was opened by ${ownerTitle}`} className="flex h-5 max-w-40 shrink-0 items-center gap-1 rounded-full bg-hover px-2 text-[11px] text-muted-foreground">
            <span aria-hidden>↑</span>
            <span className="truncate">Opened by {ownerTitle}</span>
          </span>
        )}
        {props.commands[paneKey] > 0 && !props.floated.has(paneKey) && (
          // Hiding is zooming the agent's pane: the terminals below it keep running out of sight.
          <Button
            variant="secondary"
            size="xs"
            onClick={() => actions.toggleZoom(paneKey)}
            aria-pressed={!zoomed}
            title={zoomed ? "Show the terminals this agent opened below it" : "Hide the terminals this agent opened below it. They keep running."}
            className="h-6 shrink-0 gap-1 rounded-full px-2 text-[12px] text-muted-foreground hover:text-foreground"
          >
            <SquareTerminalIcon className="size-3.5" aria-hidden />
            {props.commands[paneKey]} {zoomed ? "hidden" : props.commands[paneKey] === 1 ? "terminal" : "terminals"}
          </Button>
        )}
        <div className="flex min-w-0 flex-1 pl-2">
          <OverflowRow
            items={props.badges[paneKey] ?? []}
            keyOf={(b) => b.pluginId}
            item={({ pluginId, badge }) => <PaneBadge badge={badge} onAction={(actionId) => props.onBadgeAction(pluginId, paneKey, actionId)} />}
            more={(hidden) => (
              <Popover>
                <PopoverTrigger
                  aria-label={`${hidden.length} more`}
                  render={<Button variant="secondary" size="xs" className="h-6 shrink-0 rounded-full px-2 text-[12px] text-muted-foreground hover:text-foreground" />}
                >
                  +{hidden.length}
                </PopoverTrigger>
                <PopoverContent align="end" sideOffset={8} className="flex w-auto flex-col items-end gap-1.5 border-0 bg-popover/95 p-2 backdrop-blur-xl">
                  {hidden.map(({ pluginId, badge }) => (
                    <PaneBadge key={pluginId} badge={badge} onAction={(actionId) => props.onBadgeAction(pluginId, paneKey, actionId)} />
                  ))}
                </PopoverContent>
              </Popover>
            )}
          />
        </div>
        {owner && (
          <div className="flex items-center gap-0.5">
            <IconButton label={`Hide. It keeps running, and comes back from the button on ${ownerTitle}`} onClick={() => actions.toggleZoom(owner)}>
              <EyeOffIcon />
            </IconButton>
            <IconButton
              label={ownerSplit?.dir === "row" ? "Move below" : "Move to the right"}
              onClick={() => actions.agentView(owner, ownerSplit?.dir === "row" ? "below" : "right")}
            >
              {ownerSplit?.dir === "row" ? <PanelBottomIcon /> : <PanelRightIcon />}
            </IconButton>
            <IconButton label="Float over the agent" onClick={() => actions.agentView(owner, "float")}>
              <PictureInPicture2Icon />
            </IconButton>
          </div>
        )}
        <div
          className={cn(
            "flex items-center gap-0.5 transition-opacity duration-150",
            focused ? "opacity-70 group-hover/pane:opacity-100" : "opacity-0 group-hover/pane:opacity-100",
          )}
        >
          <IconButton label="Split right" shortcut={shortcutLabel.splitRight} onClick={() => actions.split(paneKey, "row")}>
            <Columns2Icon />
          </IconButton>
          <IconButton label="Split down" shortcut={shortcutLabel.splitDown} onClick={() => actions.split(paneKey, "column")}>
            <Rows2Icon />
          </IconButton>
          <IconButton label={zoomed ? "Restore" : "Zoom"} onClick={() => actions.toggleZoom(paneKey)}>
            {zoomed ? <Minimize2Icon /> : <Maximize2Icon />}
          </IconButton>
          <IconButton label="Close pane" shortcut={shortcutLabel.closePane} onClick={() => actions.close(paneKey)}>
            <XIcon />
          </IconButton>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent side="bottom">
        <ContextMenuItem onClick={() => (renameAfterMenu.current = true)}>
          <PencilIcon />
          Rename
        </ContextMenuItem>
        {named && (
          <ContextMenuItem onClick={() => actions.rename(paneKey, "")}>
            <SquareTerminalIcon />
            Use automatic name
          </ContextMenuItem>
        )}
        <ContextMenuSeparator />
        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <SquareTerminalIcon />
            Move to tab
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            {others.map((t) => (
              <ContextMenuItem key={t.id} onClick={() => actions.moveTo(paneKey, t.id)}>
                <span className="max-w-56 truncate">{t.title}</span>
              </ContextMenuItem>
            ))}
            {others.length > 0 && <ContextMenuSeparator />}
            <ContextMenuItem onClick={() => actions.moveTo(paneKey, "new")}>
              <SquarePlusIcon />
              New tab
            </ContextMenuItem>
          </ContextMenuSubContent>
        </ContextMenuSub>
      </ContextMenuContent>
      </ContextMenu>
      <div className="relative flex min-h-0 flex-1 flex-col">
        <div ref={host} className="min-h-0 flex-1 pr-1 pb-0.5 pl-3.5" />
        {floatedHere.length > 0 && (
          <FloatingTerminals
            ownerKey={paneKey}
            ownerTitle={label?.title ?? "its agent"}
            commands={floatedHere}
            labels={props.labels}
            visible={props.visible}
            keepRenderer={props.keepRenderer}
            initialInputs={props.initialInputs}
            onDock={() => actions.agentView(paneKey, "split")}
            onClose={actions.close}
          />
        )}
        {!!props.rails[paneKey]?.length && <PaneRails paneKey={paneKey} entries={props.rails[paneKey]} onOpen={props.onOpenStatus} />}
      </div>
      {!!props.statuses[paneKey]?.length && <PaneStatusStrip entries={props.statuses[paneKey]} onOpen={props.onOpenStatus} />}
      {session && <FindBar session={session} />}
    </motion.section>
  );
}
