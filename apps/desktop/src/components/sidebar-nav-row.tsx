import { motion } from "motion/react";
import type { ComponentProps, ReactNode } from "react";

import { cn } from "@/lib/utils";

/**
 * A quiet row at the top or bottom of the left sidebar (Search, Inbox, Settings): icon and label, muted until you
 * point at it. Its left padding matches the project rows', so the icons line up.
 */
export function SidebarNavRow({
  icon,
  label,
  trailing,
  className,
  ...props
}: Omit<ComponentProps<typeof motion.button>, "children"> & {
  icon: ReactNode;
  label: string;
  /** Right edge: a shortcut hint or a count. */
  trailing?: ReactNode;
}) {
  return (
    <motion.button
      type="button"
      whileTap={{ scale: 0.985 }}
      className={cn(
        "flex h-7 w-full shrink-0 items-center gap-2.5 rounded-lg px-2 text-left text-[13px] font-medium text-muted-foreground transition-colors duration-100 outline-none hover:bg-hover hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 data-popup-open:bg-hover data-popup-open:text-foreground [&_svg]:size-4 [&_svg]:shrink-0",
        className,
      )}
      {...props}
    >
      <span className="flex size-4 shrink-0 items-center justify-center">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {trailing}
    </motion.button>
  );
}

/** The shortcut hint on a row's right edge: plain muted text, not a keycap. */
export function RowHint({ children }: { children: ReactNode }) {
  return <span className="shrink-0 text-[11px] font-normal tracking-wide text-muted-foreground/60">{children}</span>;
}
