import { ArrowDownIcon, ArrowUpIcon, FilePenLineIcon } from "lucide-react";

import type { CountIcon } from "@/lib/plugins";
import { cn } from "@/lib/utils";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

const kinds: Record<CountIcon, { Icon: typeof ArrowUpIcon; label: (n: number) => string; tint: string }> = {
  changed: { Icon: FilePenLineIcon, label: (n) => plural(n, "changed file"), tint: "text-muted-foreground" },
  push: { Icon: ArrowUpIcon, label: (n) => `${plural(n, "commit")} to push`, tint: "text-[#9cc0ff]" },
  pull: { Icon: ArrowDownIcon, label: (n) => `${plural(n, "commit")} to pull, as of the last fetch`, tint: "text-working" },
};

/** Small icon-and-number chips for changed files and commits to push or pull. `tinted` colors each kind. */
export function GitCounts({ counts, tinted }: { counts: { icon: CountIcon; value: number }[]; tinted?: boolean }) {
  if (counts.length === 0) return null;
  return (
    <span className="flex shrink-0 items-center gap-1.5 text-[11px] leading-4 tabular-nums">
      {counts.map(({ icon, value }) => {
        const { Icon, label, tint } = kinds[icon];
        return (
          <span key={icon} title={label(value)} aria-label={label(value)} className={cn("flex items-center gap-0.5", tinted && tint)}>
            <Icon className="size-3" aria-hidden />
            {value}
          </span>
        );
      })}
    </span>
  );
}
