import { useEffect, useMemo, useRef, useState } from "react";
import { CheckIcon, ChevronDownIcon, CopyIcon, EllipsisIcon, GitBranchIcon, GitForkIcon, SearchIcon, SearchXIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { api, type HistoryHit, type HistoryResults, type HistorySnippet, type Space } from "@/lib/api";
import { shellQuote } from "@/lib/resume";

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

function ago(ms: number) {
  const minutes = Math.round((ms - Date.now()) / 60_000);
  if (Math.abs(minutes) < 60) return relative.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return relative.format(hours, "hour");
  return relative.format(Math.round(hours / 24), "day");
}

const PAGE = 100;
const DAY = 86_400_000;
type Scope = "project" | "all";
type Since = "any" | "today" | "week" | "month";
const SINCE: Record<Since, { label: string; ms: () => number | null }> = {
  any: { label: "Any time", ms: () => null },
  today: { label: "Today", ms: () => new Date().setHours(0, 0, 0, 0) },
  week: { label: "7 days", ms: () => Date.now() - 7 * DAY },
  month: { label: "30 days", ms: () => Date.now() - 30 * DAY },
};

/** The words and "quoted phrases" the search matched, as the Rust side splits them. */
function searchTerms(query: string) {
  return query
    .split('"')
    .flatMap((part, i) => (i % 2 ? [part.trim().split(/\s+/).join(" ")] : part.split(/\s+/)))
    .filter(Boolean);
}

function Highlight({ text, terms }: { text: string; terms: string[] }) {
  if (!terms.length) return text;
  const pattern = new RegExp(`(${terms.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`, "gi");
  return text.split(pattern).map((part, i) =>
    i % 2 ? (
      <mark key={i} className="rounded-[3px] bg-working/25 px-px text-foreground">
        {part}
      </mark>
    ) : (
      part
    ),
  );
}

const SPEAKER: Record<HistorySnippet["role"], string> = { user: "You", assistant: "Claude", file: "Edited" };

export function HistorySheet(props: {
  space: Space | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  liveSessionIds: Set<string>;
  /** Resumes a session that started in the current project's folder. */
  onResume: (sessionId: string) => void;
  /** Resumes or forks a session that started in `cwd`, which can be another project or a folder inside this one. */
  onLaunch: (session: { id: string; cwd: string }, fork: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<Scope>("project");
  const [since, setSince] = useState<Since>("any");
  const [branch, setBranch] = useState<string | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const [results, setResults] = useState<HistoryResults | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const latest = useRef(0);
  const space = props.space;
  const project = scope === "project" ? (space?.path ?? null) : null;
  const terms = useMemo(() => searchTerms(query), [query]);

  // Another project's results and branches don't apply here.
  useEffect(() => {
    setResults(null);
    setBranch(null);
    setLimit(PAGE);
  }, [project]);

  useEffect(() => {
    if (!props.open) return;
    const request = ++latest.current;
    setSearching(true);
    setError(null);
    // Typing waits for a pause, so a search runs once per burst of keys, not per key.
    const timer = setTimeout(
      () =>
        void api
          .historySearch(query, { project, branch, sinceMs: SINCE[since].ms() }, limit)
          .then((r) => request === latest.current && setResults(r))
          .catch((e) => request === latest.current && setError(String(e instanceof Error ? e.message : e)))
          .finally(() => request === latest.current && setSearching(false)),
      query ? 150 : 0,
    );
    return () => clearTimeout(timer);
  }, [props.open, query, project, branch, since, limit]);

  const copy = (hit: HistoryHit) => {
    const cwd = hit.cwd ?? space?.path;
    const command = `${cwd ? `cd ${shellQuote(cwd)} && ` : ""}claude --resume ${hit.id}`;
    void navigator.clipboard.writeText(command).then(() => {
      setCopied(hit.id);
      setTimeout(() => setCopied((id) => (id === hit.id ? null : id)), 1500);
    });
  };

  const resume = (hit: HistoryHit, fork: boolean) => {
    const cwd = hit.cwd ?? (scope === "project" ? space?.path : null);
    if (!cwd) return;
    if (!fork && cwd === space?.path) props.onResume(hit.id);
    else props.onLaunch({ id: hit.id, cwd }, fork);
  };

  const where = (hit: HistoryHit) => {
    if (!hit.cwd || hit.cwd === space?.path) return null;
    if (scope === "project" && space && hit.cwd.startsWith(`${space.path}/`)) return hit.cwd.slice(space.path.length + 1);
    return hit.cwd.split("/").filter(Boolean).at(-1) ?? hit.cwd;
  };

  const sessions = results?.sessions;
  const filtered = Boolean(query.trim() || branch || since !== "any");

  return (
    <Sheet open={props.open} onOpenChange={props.onOpenChange}>
      <SheetContent side="right" className="gap-0 border-hairline bg-popover/85 backdrop-blur-2xl data-[side=right]:w-[480px] data-[side=right]:sm:max-w-[480px]">
        <SheetHeader className="gap-3 border-b border-hairline pb-3">
          <div className="flex flex-col gap-0.5">
            <SheetTitle>Past sessions</SheetTitle>
            <SheetDescription>
              {project ? `Claude Code sessions in ${space?.name ?? "this project"}.` : "Claude Code sessions in every project."}
            </SheetDescription>
          </div>
          <ToggleGroup
            aria-label="Which sessions"
            size="sm"
            value={[project ? "project" : "all"]}
            onValueChange={(value) => value[0] && setScope(value[0] as Scope)}
            className="rounded-lg bg-white/[0.05] p-0.5"
          >
            <ToggleGroupItem value="project" disabled={!space} className="data-pressed:bg-white/[0.09]">
              This project
            </ToggleGroupItem>
            <ToggleGroupItem value="all" className="data-pressed:bg-white/[0.09]">
              All projects
            </ToggleGroupItem>
          </ToggleGroup>
          <InputGroup>
            <InputGroupAddon>
              <SearchIcon aria-hidden />
            </InputGroupAddon>
            <InputGroupInput
              autoFocus
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setLimit(PAGE);
              }}
              placeholder="Search prompts, replies, titles, files"
              aria-label="Search past sessions"
            />
            {searching && sessions && (
              <InputGroupAddon align="inline-end">
                <Spinner aria-label="Searching" />
              </InputGroupAddon>
            )}
          </InputGroup>
          <div className="flex items-center justify-between gap-2">
            <ToggleGroup
              aria-label="Last active"
              size="sm"
              value={[since]}
              onValueChange={(value) => {
                if (!value[0]) return;
                setSince(value[0] as Since);
                setLimit(PAGE);
              }}
              className="rounded-lg bg-white/[0.05] p-0.5"
            >
              {(Object.keys(SINCE) as Since[]).map((key) => (
                <ToggleGroupItem key={key} value={key} className="text-[12px] data-pressed:bg-white/[0.09]">
                  {SINCE[key].label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={<Button variant="ghost" size="sm" className="min-w-0 text-[12px] text-muted-foreground hover:text-foreground" />}
              >
                <GitBranchIcon aria-hidden />
                <span className="truncate">{branch ?? "All branches"}</span>
                <ChevronDownIcon aria-hidden />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-auto max-w-72">
                <DropdownMenuRadioGroup
                  value={branch ?? ""}
                  onValueChange={(value: string) => {
                    setBranch(value || null);
                    setLimit(PAGE);
                  }}
                >
                  <DropdownMenuRadioItem value="" closeOnClick>
                    All branches
                  </DropdownMenuRadioItem>
                  {!!results?.branches.length && <DropdownMenuSeparator />}
                  {results?.branches.map((b) => (
                    <DropdownMenuRadioItem key={b} value={b} closeOnClick>
                      <span className="truncate">{b}</span>
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </SheetHeader>
        <ScrollArea className="min-h-0 flex-1">
          {error ? (
            <Empty className="py-16">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <SearchXIcon aria-hidden />
                </EmptyMedia>
                <EmptyTitle>Couldn't read past sessions</EmptyTitle>
                <EmptyDescription>{error}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : !sessions ? (
            <div className="flex flex-col gap-1 p-2" aria-busy="true" aria-label="Reading transcripts">
              {Array.from({ length: 6 }, (_, i) => (
                <div key={i} className="flex flex-col gap-1.5 p-2.5">
                  <Skeleton className="h-4 bg-white/[0.06]" style={{ width: `${50 + ((i * 29) % 40)}%` }} />
                  <Skeleton className="h-3 bg-white/[0.04]" style={{ width: `${70 + ((i * 17) % 25)}%` }} />
                  <Skeleton className="h-3 w-24 bg-white/[0.04]" />
                </div>
              ))}
            </div>
          ) : sessions.length === 0 ? (
            <Empty className="py-16">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <SearchXIcon aria-hidden />
                </EmptyMedia>
                <EmptyTitle>{filtered ? "No session matches" : "No Claude sessions yet"}</EmptyTitle>
                <EmptyDescription>
                  {filtered
                    ? "Every word has to appear in the session. Try fewer words, another branch or a longer time."
                    : project
                      ? "Sessions you run in this project show up here."
                      : "Sessions you run with Claude Code show up here."}
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <ul className="flex flex-col p-2">
              {sessions.map((s) => {
                const live = props.liveSessionIds.has(s.id);
                const snippet = s.snippets[0];
                const place = where(s);
                const launchable = Boolean(s.cwd ?? (project && space?.path));
                return (
                  <li key={s.id} className="group/session flex items-start gap-2 rounded-lg p-2.5 transition-colors duration-150 hover:bg-hover">
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">{s.title ?? s.firstPrompt ?? "Untitled session"}</p>
                      {snippet ? (
                        <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">
                          <span className="text-foreground/70">{SPEAKER[snippet.role]}: </span>
                          <Highlight text={snippet.text} terms={terms} />
                        </p>
                      ) : (
                        s.title &&
                        s.firstPrompt && <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">{s.firstPrompt}</p>
                      )}
                      <p className="mt-1 truncate text-xs text-muted-foreground">
                        {ago(s.lastActiveMs)}
                        {s.gitBranch && <> on {s.gitBranch}</>}
                        {place && <> in {place}</>}
                        {s.matches > 1 && <> · {s.matches} matching messages</>}
                      </p>
                    </div>
                    <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity duration-150 group-hover/session:opacity-100 focus-within:opacity-100 has-data-popup-open:opacity-100">
                      <Button
                        size="sm"
                        variant={live ? "ghost" : "secondary"}
                        disabled={live || !launchable}
                        onClick={() => resume(s, false)}
                        className="rounded-full px-3 disabled:opacity-60"
                      >
                        {live ? "Running" : "Resume"}
                      </Button>
                      <DropdownMenu>
                        <DropdownMenuTrigger
                          render={<Button variant="ghost" size="icon-sm" aria-label="More for this session" className="text-muted-foreground" />}
                        >
                          <EllipsisIcon aria-hidden />
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-auto">
                          <DropdownMenuItem disabled={!launchable} onClick={() => resume(s, true)}>
                            <GitForkIcon aria-hidden />
                            Fork into a new session
                          </DropdownMenuItem>
                          <DropdownMenuItem closeOnClick={false} onClick={() => copy(s)}>
                            {copied === s.id ? <CheckIcon aria-hidden /> : <CopyIcon aria-hidden />}
                            {copied === s.id ? "Copied" : "Copy resume command"}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </li>
                );
              })}
              {results.total > sessions.length && (
                <li className="flex items-center justify-between gap-2 px-2.5 py-2 text-xs text-muted-foreground">
                  Showing {sessions.length} of {results.total}
                  <Button size="sm" variant="ghost" onClick={() => setLimit((n) => n + PAGE)}>
                    Show more
                  </Button>
                </li>
              )}
            </ul>
          )}
        </ScrollArea>
      </SheetContent>
    </Sheet>
  );
}
