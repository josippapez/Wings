import { TerminalIcon } from "lucide-react";

import { ClaudeIcon } from "@/components/claude-icon";
import { OpencodeIcon } from "@/components/opencode-icon";
import { StatusDot } from "@/components/status-dot";
import { paneGlyph } from "@/lib/agents";
import type { Agent, AgentKind, PaneInfo, Space } from "@/lib/api";

/** `/Users/me/x` → `~/x` (also `/home/me` and `C:\Users\me`). */
export function tildify(path: string) {
  return path.replace(/^(\/Users\/[^/]+|\/home\/[^/]+|[A-Za-z]:\\Users\\[^\\]+)/, "~");
}

/** `running` is the program a pane without an agent runs instead of sitting at its shell prompt, like `node`. */
export type PaneLabelInfo = { title: string; detail: string; agent: Agent | undefined; running?: string };

/** The name shown for an agent that has none of its own. */
export function agentLabel(kind: AgentKind) {
  return kind === "opencode" ? "opencode" : "Claude";
}

export function paneLabel(agent: Agent | undefined, info: PaneInfo | undefined, space: Space | undefined): PaneLabelInfo {
  if (agent) return { title: agent.name ?? agentLabel(agent.kind), detail: space?.name ?? "", agent };
  const cwd = info?.cwd ?? space?.path;
  const running = info?.busy && info.command ? info.command : undefined;
  return { title: info?.command || "shell", detail: cwd ? tildify(cwd) : "", agent, running };
}

/** An idle agent shows its own mark and a busy one its live status (same for Claude and opencode); plain shells get a terminal glyph. */
export function PaneIcon({ agent, className }: { agent: Agent | undefined; className?: string }) {
  switch (paneGlyph(agent)) {
    case "shell":
      return <TerminalIcon className={className ?? "size-3.5 text-muted-foreground"} aria-hidden />;
    case "claude":
      return <ClaudeIcon className={className ?? "size-3.5 text-[#d97757]"} aria-hidden />;
    case "opencode":
      return <OpencodeIcon className={className ?? "size-3.5 text-foreground"} aria-hidden />;
    default:
      return <StatusDot state={agent!.state} />;
  }
}
