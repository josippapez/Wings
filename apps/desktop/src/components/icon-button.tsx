import type { ComponentProps, ReactNode } from "react";
import { motion } from "motion/react";

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/** Small square button for chrome: soft hover, a press that gives, and a tooltip with its shortcut. */
export function IconButton({
  label,
  shortcut,
  className,
  children,
  side = "bottom",
  ...props
}: ComponentProps<typeof motion.button> & {
  label: string;
  shortcut?: string;
  side?: "top" | "bottom" | "left" | "right";
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <motion.button
            type="button"
            aria-label={label}
            whileTap={{ scale: 0.88 }}
            transition={{ type: "spring", stiffness: 600, damping: 30 }}
            className={cn(
              "inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none transition-colors duration-150 hover:bg-hover hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring active:bg-pressed [&_svg]:size-4",
              className,
            )}
            {...props}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent side={side} className="gap-2">
        {label}
        {shortcut && <kbd className="font-sans text-[11px] opacity-60">{shortcut}</kbd>}
      </TooltipContent>
    </Tooltip>
  );
}
