import { useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  CircleAlertIcon,
  GitMergeIcon,
  GitPullRequestClosedIcon,
  GitPullRequestDraftIcon,
  GitPullRequestIcon,
} from "lucide-react";

import { GitCounts } from "@/components/git-counts";
import { badgeVariants } from "@/components/ui/badge";
import { buttonVariants } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Spinner } from "@/components/ui/spinner";
import type { Badge, BadgeIcon, Tone } from "@/lib/plugins";
import { cn } from "@/lib/utils";

const toneClass: Record<Tone, { pill: string; text: string; dot: string }> = {
  neutral: { pill: "bg-white/[0.06] text-muted-foreground ring-white/10", text: "text-muted-foreground", dot: "bg-idle" },
  info: { pill: "bg-[#7aa7f2]/12 text-[#9cc0ff] ring-[#7aa7f2]/25", text: "text-[#9cc0ff]", dot: "bg-[#7aa7f2]" },
  success: { pill: "bg-done/12 text-done ring-done/25", text: "text-done", dot: "bg-done" },
  warning: { pill: "bg-working/12 text-working ring-working/25", text: "text-working", dot: "bg-working" },
  danger: { pill: "bg-blocked/12 text-blocked ring-blocked/30", text: "text-blocked", dot: "bg-blocked" },
  merged: { pill: "bg-[#b392f0]/14 text-[#c9b2f7] ring-[#b392f0]/30", text: "text-[#c9b2f7]", dot: "bg-[#b392f0]" },
};

const iconFor: Record<BadgeIcon, typeof GitPullRequestIcon> = {
  "pr-open": GitPullRequestIcon,
  "pr-merged": GitMergeIcon,
  "pr-closed": GitPullRequestClosedIcon,
  "pr-draft": GitPullRequestDraftIcon,
};

/** A plugin's badge in a pane header: a small pill that opens a card with details and actions. */
export function PaneBadge({ badge, onAction }: { badge: Badge; onAction: (actionId: string) => Promise<void> }) {
  const tone = toneClass[badge.tone];
  const Icon = badge.icon ? iconFor[badge.icon] : null;
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function run(actionId: string) {
    setPending(actionId);
    setError(null);
    try {
      await onAction(actionId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPending(null);
    }
  }

  return (
    <Popover>
      <PopoverTrigger
        render={
          <motion.button
            type="button"
            layout
            whileTap={{ scale: 0.94 }}
            transition={{ type: "spring", stiffness: 500, damping: 32 }}
            className={cn(
              badgeVariants(),
              "h-6 cursor-pointer gap-1.5 px-2.5 text-[12px] ring-1 ring-inset hover:brightness-125 aria-expanded:brightness-125 [&>svg]:size-3.5!",
              tone.pill,
            )}
          />
        }
      >
        {badge.loading ? (
          <Spinner aria-label="Updating" />
        ) : (
          Icon && <Icon className="size-3.5" aria-hidden />
        )}
        <span className="max-w-44 truncate">{badge.label}</span>
        {badge.counts && <GitCounts counts={badge.counts} />}
      </PopoverTrigger>
      <PopoverContent align="end" sideOffset={8} className="w-80 gap-3 border-0 bg-popover/95 p-3.5 backdrop-blur-xl">
        <div className="flex items-start gap-2.5">
          {Icon && <Icon className={cn("mt-0.5 size-4 shrink-0", tone.text)} aria-hidden />}
          <div className="min-w-0">
            {badge.title && <p className="line-clamp-2 text-[13px] leading-snug font-semibold">{badge.title}</p>}
            {badge.subtitle && <p className="mt-0.5 truncate text-[12px] text-muted-foreground">{badge.subtitle}</p>}
          </div>
        </div>
        {!!badge.rows?.length && (
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 rounded-lg bg-white/[0.03] p-2.5 text-[12px]">
            {badge.rows.map((row) => (
              <div key={row.label} className="contents">
                <dt className="text-muted-foreground">{row.label}</dt>
                <dd className={cn("flex min-w-0 items-center gap-1.5", row.tone && toneClass[row.tone].text)}>
                  {row.tone && <span className={cn("size-1.5 shrink-0 rounded-full", toneClass[row.tone].dot)} aria-hidden />}
                  <span className="truncate" title={row.value}>
                    {row.value}
                  </span>
                </dd>
              </div>
            ))}
          </dl>
        )}
        <AnimatePresence initial={false}>
          {error && (
            <motion.p
              role="alert"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              className="flex items-start gap-1.5 overflow-hidden text-[12px] text-blocked"
            >
              <CircleAlertIcon className="mt-px size-3.5 shrink-0" aria-hidden />
              {error}
            </motion.p>
          )}
        </AnimatePresence>
        {!!badge.actions?.length && (
          <div className="flex flex-wrap gap-1.5">
            {badge.actions.map((action) => (
              <motion.button
                key={action.id}
                type="button"
                whileTap={{ scale: 0.96 }}
                disabled={pending !== null}
                aria-busy={pending === action.id}
                onClick={() => void run(action.id)}
                className={cn(
                  buttonVariants({ variant: action.primary ? "default" : "secondary", size: "sm" }),
                  "rounded-full px-3 text-[12px] disabled:cursor-default disabled:opacity-100",
                  pending !== null && pending !== action.id && "opacity-50",
                )}
              >
                {pending === action.id && <Spinner className="size-3.5" />}
                {action.label}
              </motion.button>
            ))}
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
