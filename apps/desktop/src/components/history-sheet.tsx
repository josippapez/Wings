import { useEffect, useMemo, useState } from "react";
import { SearchIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { api, type SessionSummary, type Space } from "@/lib/api";

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

function ago(ms: number) {
  const minutes = Math.round((ms - Date.now()) / 60_000);
  if (Math.abs(minutes) < 60) return relative.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return relative.format(hours, "hour");
  return relative.format(Math.round(hours / 24), "day");
}

export function HistorySheet(props: {
  space: Space | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  liveSessionIds: Set<string>;
  onResume: (sessionId: string) => void;
}) {
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [query, setQuery] = useState("");
  const spaceId = props.space?.id;

  useEffect(() => {
    if (!props.open || !spaceId) return;
    setSessions(null);
    void api.sessionsList(spaceId).then(setSessions);
  }, [props.open, spaceId]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q || !sessions) return sessions;
    return sessions.filter((s) =>
      [s.title, s.firstPrompt, s.gitBranch, s.id].some((field) => field?.toLowerCase().includes(q)),
    );
  }, [sessions, query]);

  return (
    <Sheet open={props.open} onOpenChange={props.onOpenChange}>
      <SheetContent side="right" className="w-[440px] gap-0 border-hairline bg-popover/85 backdrop-blur-2xl sm:max-w-[440px]">
        <SheetHeader className="border-b border-hairline pb-3">
          <SheetTitle>Past sessions</SheetTitle>
          <SheetDescription>Claude Code sessions in {props.space?.name ?? "this project"}.</SheetDescription>
          <div className="relative mt-3">
            <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search titles, prompts, branches"
              aria-label="Search past sessions"
              className="pl-8"
            />
          </div>
        </SheetHeader>
        <ScrollArea className="min-h-0 flex-1">
          <ul className="flex flex-col p-2">
            {filtered === null && <li className="p-3 text-sm text-muted-foreground">Reading transcripts…</li>}
            {filtered?.length === 0 && (
              <li className="p-3 text-sm text-muted-foreground">
                {query ? "No session matches that search." : "No Claude sessions in this project yet."}
              </li>
            )}
            {filtered?.map((s) => {
              const live = props.liveSessionIds.has(s.id);
              return (
                <li key={s.id} className="group/session flex items-start gap-3 rounded-lg p-2.5 transition-colors duration-150 hover:bg-hover">
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium">{s.title ?? s.firstPrompt ?? "Untitled session"}</p>
                    {s.title && s.firstPrompt && (
                      <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">{s.firstPrompt}</p>
                    )}
                    <p className="mt-1 text-xs text-muted-foreground">
                      {ago(s.lastActiveMs)}
                      {s.gitBranch && <> on {s.gitBranch}</>}
                    </p>
                  </div>
                  <Button
                    size="sm"
                    variant={live ? "ghost" : "secondary"}
                    disabled={live}
                    onClick={() => props.onResume(s.id)}
                    className="rounded-full px-3 opacity-0 transition-opacity duration-150 group-hover/session:opacity-100 focus-visible:opacity-100 disabled:opacity-60"
                  >
                    {live ? "Running" : "Resume"}
                  </Button>
                </li>
              );
            })}
          </ul>
        </ScrollArea>
      </SheetContent>
    </Sheet>
  );
}
