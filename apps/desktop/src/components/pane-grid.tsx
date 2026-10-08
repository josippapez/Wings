import { useEffect, useRef } from "react";
import { motion } from "motion/react";
import { Columns2Icon, Maximize2Icon, Minimize2Icon, Rows2Icon, XIcon } from "lucide-react";

import { IconButton } from "@/components/icon-button";
import { OverflowRow } from "@/components/overflow-row";
import { PaneBadge } from "@/components/pane-badge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { PaneIcon, type PaneLabelInfo } from "@/components/pane-label";
import type { LayoutNode } from "@/lib/layout";
import type { Badge } from "@/lib/plugins";
import { shortcutLabel, terminals } from "@/lib/terminal";
import { cn } from "@/lib/utils";

export type PaneActions = {
  focus: (paneKey: string) => void;
  split: (paneKey: string, dir: "row" | "column") => void;
  close: (paneKey: string) => void;
  toggleZoom: (paneKey: string) => void;
  resize: (splitId: string, ratio: number) => void;
};

type GridProps = {
  layout: LayoutNode;
  visible: boolean;
  focusedPane: string;
  zoomedPane: string | null;
  labels: Record<string, PaneLabelInfo>;
  initialInputs: Record<string, string | null>;
  badges: Record<string, { pluginId: string; badge: Badge }[]>;
  onBadgeAction: (pluginId: string, paneKey: string, actionId: string) => Promise<void>;
  actions: PaneActions;
};

/** One tab's panes. Every tab stays mounted (hidden when inactive) so terminals never lose their DOM. */
export function PaneGrid(props: GridProps) {
  const root = props.zoomedPane ? ({ kind: "pane", id: props.zoomedPane } as const) : props.layout;
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
            "absolute rounded-full bg-white/0 transition-colors duration-150 group-hover/divider:bg-white/25 group-active/divider:bg-white/40",
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

  // A split restructures the tree and remounts this card, so the terminal re-attaches to the new host here.
  useEffect(() => {
    const term = terminals.get(paneKey);
    if (term && host.current) term.attach(host.current, props.initialInputs[paneKey] ?? null);
  }, [paneKey, props.initialInputs]);

  useEffect(() => {
    terminals.get(paneKey)?.setVisible(props.visible);
  }, [paneKey, props.visible]);

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
      <div className="flex h-9 shrink-0 items-center gap-2 pr-1.5 pl-3.5">
        <PaneIcon agent={label?.agent} />
        <span className={cn("max-w-[50%] truncate text-[13px] font-semibold", !focused && "text-muted-foreground")}>
          {label?.title}
        </span>
        {label?.detail && <span className="min-w-0 truncate text-[12px] text-muted-foreground">{label.detail}</span>}
        <div className="flex min-w-0 flex-1 pl-2">
          <OverflowRow
            items={props.badges[paneKey] ?? []}
            keyOf={(b) => b.pluginId}
            item={({ pluginId, badge }) => <PaneBadge badge={badge} onAction={(actionId) => props.onBadgeAction(pluginId, paneKey, actionId)} />}
            more={(hidden) => (
              <Popover>
                <PopoverTrigger
                  aria-label={`${hidden.length} more`}
                  className="inline-flex h-6 shrink-0 items-center rounded-full bg-white/[0.06] px-2 text-[12px] font-medium text-muted-foreground ring-1 ring-white/10 ring-inset transition-colors outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
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
      </div>
      <div ref={host} className="min-h-0 flex-1 pr-1 pb-2 pl-3.5" />
    </motion.section>
  );
}
