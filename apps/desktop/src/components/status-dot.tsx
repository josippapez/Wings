import { cn } from "@/lib/utils";
import type { AgentState } from "@/lib/api";

export const stateLabel: Record<AgentState, string> = {
  working: "Working",
  blocked: "Needs you",
  done: "Done",
  idle: "Idle",
};

/** Priority for rolling several agents up into one dot: whatever needs you most wins. */
const rank: Record<AgentState, number> = { blocked: 3, done: 2, working: 1, idle: 0 };

export function rollUp(states: AgentState[]): AgentState | null {
  return states.reduce<AgentState | null>((best, s) => (best === null || rank[s] > rank[best] ? s : best), null);
}

export function StatusDot({ state, className }: { state: AgentState | null; className?: string }) {
  return (
    <span
      role="img"
      aria-label={state ? stateLabel[state] : "No agent"}
      className={cn("relative inline-flex size-2.5 shrink-0 items-center justify-center", className)}
    >
      {state === "working" && (
        <span className="size-2.5 rounded-full border-[1.5px] border-working/30 border-t-working motion-safe:animate-spin" />
      )}
      {state === "blocked" && (
        <>
          <span className="absolute size-2.5 rounded-full bg-blocked/50 motion-safe:animate-ping" />
          <span className="size-2.5 rounded-full bg-blocked" />
        </>
      )}
      {state === "done" && <span className="size-2 rounded-full bg-done" />}
      {state === "idle" && <span className="size-2 rounded-full border-[1.5px] border-idle" />}
      {state === null && <span className="size-1 rounded-full bg-muted-foreground/40" />}
    </span>
  );
}
