import {
  ActivityIcon,
  CalendarIcon,
  ChartColumnIcon,
  ClockIcon,
  FolderTreeIcon,
  GaugeIcon,
  GitBranchIcon,
  GitCompareArrowsIcon,
  GitGraphIcon,
  GlobeIcon,
  ListIcon,
  ListTodoIcon,
  MessageSquareTextIcon,
  PuzzleIcon,
  ScrollTextIcon,
  ServerIcon,
  SquareKanbanIcon,
  SquareTerminalIcon,
  TimerIcon,
} from "lucide-react";

import names from "./plugin-icons.json" with { type: "json" };

/** The icons a plugin's sidebar, view or panel may name in its manifest. Rust checks manifests against the same list. */
export const pluginIcons: Record<string, typeof ClockIcon> = {
  clock: ClockIcon,
  globe: GlobeIcon,
  calendar: CalendarIcon,
  chart: ChartColumnIcon,
  list: ListIcon,
  "git-compare": GitCompareArrowsIcon,
  "git-graph": GitGraphIcon,
  "git-branch": GitBranchIcon,
  "folder-tree": FolderTreeIcon,
  message: MessageSquareTextIcon,
  scroll: ScrollTextIcon,
  kanban: SquareKanbanIcon,
  gauge: GaugeIcon,
  activity: ActivityIcon,
  timer: TimerIcon,
  server: ServerIcon,
  terminal: SquareTerminalIcon,
  todo: ListTodoIcon,
  puzzle: PuzzleIcon,
};

export const pluginIconNames: readonly string[] = names;
