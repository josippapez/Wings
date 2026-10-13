import { GlobeIcon, PuzzleIcon } from "lucide-react";

import { iconMaskStyle } from "@/lib/plugin-icon-mask";
import { pluginIcons } from "@/lib/plugin-icons";
import { cn } from "@/lib/utils";

/**
 * A plugin's icon for a button, tab or menu item: the built-in icon its manifest names, else the plugin's own SVG
 * (drawn in the text colour), else a globe for an unknown name or a puzzle piece when it names none.
 */
export function PluginGlyph({ name, svg, className }: { name: string; svg?: string | null; className?: string }) {
  const Icon = pluginIcons[name];
  if (Icon) return <Icon className={className} aria-hidden />;
  if (svg) return <span aria-hidden className={cn("inline-block size-4 shrink-0 bg-current", className)} style={iconMaskStyle(svg)} />;
  const Fallback = name ? GlobeIcon : PuzzleIcon;
  return <Fallback className={className} aria-hidden />;
}
