import { PlusIcon } from "lucide-react";

import { cn } from "@/lib/utils";
import { PluginGlyph } from "@/components/plugin-glyph";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import type { BlockEntry } from "@/lib/plugin-blocks";

/** The "+ Add" row between two items: a hairline with a button that shows on hover or focus and offers a section, a fact or a plugin's block. */
export function AddRow({ label, blocks, canSection, canBlock, onSection, onFact, onBlock, always }: { label: string; blocks: BlockEntry[]; canSection: boolean; canBlock: boolean; /** Shown without hover, for the row at the end of the page. */ always?: boolean; onSection: () => void; onFact: () => void; onBlock: (entry: BlockEntry) => void }) {
  return (
    <div className={cn("group/add relative flex items-center justify-center", always ? "h-9" : "h-7")}>
      <span aria-hidden className={cn("absolute inset-x-0 top-1/2 h-px bg-hairline opacity-0 transition-opacity group-focus-within/add:opacity-100 group-hover/add:opacity-100", always && "hidden")} />
      <DropdownMenu>
        <DropdownMenuTrigger
          aria-label={label}
          className={cn(
            "relative inline-flex h-6 items-center gap-1 rounded-full border bg-surface px-2.5 text-[11px] text-muted-foreground transition-[opacity,color,border-color] outline-none hover:border-hairline-strong hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring/50 aria-expanded:opacity-100",
            always ? "border-dashed border-hairline-strong opacity-100" : "border-hairline opacity-0 group-hover/add:opacity-100",
          )}
        >
          <PlusIcon className="size-3" aria-hidden />
          Add
        </DropdownMenuTrigger>
        <DropdownMenuContent align="center">
          <DropdownMenuItem disabled={!canSection} onClick={onSection}>
            Section
          </DropdownMenuItem>
          <DropdownMenuItem onClick={onFact}>Fact</DropdownMenuItem>
          {blocks.length > 0 && <DropdownMenuSeparator />}
          {blocks.map((b) => (
            <DropdownMenuItem key={b.key} disabled={!canBlock} onClick={() => onBlock(b)}>
              <PluginGlyph name={b.icon} svg={b.iconSvg} />
              {b.title}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
