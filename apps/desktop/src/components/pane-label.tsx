import { TerminalIcon } from "lucide-react";

import { CodexIcon, CursorIcon, GrokIcon } from "@/components/agent-icons";
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

/** The name shown for an agent that has none of its own; the same names Settings > Agents uses. */
const AGENT_LABEL: Record<AgentKind, string> = { claude: "Claude", opencode: "opencode", codex: "Codex", cursor: "Cursor CLI", grok: "Grok Build" };

export function agentLabel(kind: AgentKind) {
  return AGENT_LABEL[kind];
}

export function paneLabel(agent: Agent | undefined, info: PaneInfo | undefined, space: Space | undefined): PaneLabelInfo {
  if (agent) return { title: agent.name ?? agentLabel(agent.kind), detail: space?.name ?? "", agent };
  const cwd = info?.cwd ?? space?.path;
  const running = info?.busy && info.command ? info.command : undefined;
  return { title: info?.command || "shell", detail: cwd ? tildify(cwd) : "", agent, running };
}

/** Set showState to false where the status is shown separately. */
export function PaneIcon({ agent, className, showState = true }: { agent: Agent | undefined; className?: string; showState?: boolean }) {
  switch (paneGlyph(agent, showState)) {
    case "shell":
      return <TerminalIcon className={className ?? "size-3.5 text-muted-foreground"} aria-hidden />;
    case "claude":
      return <ClaudeIcon className={className ?? "size-3.5 text-[#d97757]"} aria-hidden />;
    case "opencode":
      return <OpencodeIcon className={className ?? "size-3.5 text-foreground"} aria-hidden />;
    case "codex":
      return <CodexIcon className={className ?? "size-3.5 text-foreground"} aria-hidden />;
    case "cursor":
      return <CursorIcon className={className ?? "size-3.5 text-foreground"} aria-hidden />;
    case "grok":
      return <GrokIcon className={className ?? "size-3.5 text-foreground"} aria-hidden />;
    default:
      return <StatusDot state={agent!.state} />;
  }
}
