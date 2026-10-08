import { AsteriskIcon, TerminalIcon } from "lucide-react";

import { StatusDot } from "@/components/status-dot";
import type { Agent, PaneInfo, Space } from "@/lib/api";

/** `/Users/me/x` → `~/x` (also `/home/me` and `C:\Users\me`). */
export function tildify(path: string) {
  return path.replace(/^(\/Users\/[^/]+|\/home\/[^/]+|[A-Za-z]:\\Users\\[^\\]+)/, "~");
}

export type PaneLabelInfo = { title: string; detail: string; agent: Agent | undefined };

export function paneLabel(agent: Agent | undefined, info: PaneInfo | undefined, space: Space | undefined): PaneLabelInfo {
  if (agent) return { title: agent.name ?? "Claude", detail: space?.name ?? "", agent };
  const cwd = info?.cwd ?? space?.path;
  return { title: info?.command || "shell", detail: cwd ? tildify(cwd) : "", agent };
}

/** Claude panes get their live status; plain shells get a terminal glyph. */
export function PaneIcon({ agent, className }: { agent: Agent | undefined; className?: string }) {
  if (!agent) return <TerminalIcon className={className ?? "size-3.5 text-muted-foreground"} aria-hidden />;
  if (agent.state === "idle") return <AsteriskIcon className={className ?? "size-3.5 text-[#d97757]"} aria-hidden />;
  return <StatusDot state={agent.state} />;
}
