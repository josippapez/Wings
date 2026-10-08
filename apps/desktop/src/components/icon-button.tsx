import type { ComponentProps, ReactNode } from "react";
import { motion } from "motion/react";

import { buttonVariants } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
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
          // shadcn's ghost icon button look, on a motion button for the press.
          <motion.button
            type="button"
            aria-label={label}
            whileTap={{ scale: 0.88 }}
            transition={{ type: "spring", stiffness: 600, damping: 30 }}
            className={cn(
              buttonVariants({ variant: "ghost", size: "icon-sm" }),
              "shrink-0 text-muted-foreground hover:bg-hover hover:text-foreground active:bg-pressed [&_svg:not([class*='size-'])]:size-4",
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
        {shortcut && <Kbd>{shortcut}</Kbd>}
      </TooltipContent>
    </Tooltip>
  );
}
