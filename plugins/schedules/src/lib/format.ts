// What the `list` tool prints: plain text, one line for each entry, ticket and waiting ticket. No `wings`, so node tests can drive it.

import { asks, describeCheck, SOURCE_LABELS, startedElsewhere, type Watch } from "./assigned";
import { describeWhen, nameOf, type Schedule } from "./schedule";

export function formatList(schedules: Schedule[], watches: Watch[], projectName: (id: string) => string, now: number): string {
  const lines: string[] = [];
  for (const s of schedules) {
    lines.push(`${s.id} · schedule · ${projectName(s.projectId)} · ${s.agent} · ${s.enabled ? "enabled" : "paused"} · ${nameOf(s)} · ${describeWhen(s.when)}`);
  }
  for (const w of watches) {
    const check = w.lastCheck;
    const checked = check ? `checked ${Math.max(0, Math.round((now - check.at) / 60_000))} min ago: ${describeCheck(check, w.source)}` : describeCheck(check, w.source);
    const which = w.states === undefined ? "states not picked yet" : w.states.length ? `asks about ${w.states.join(", ")}` : "asks about every state";
    const titles = w.prefixes?.length ? ` · only titles starting with ${w.prefixes.join(", ")}` : "";
    lines.push(`${w.id} · assigned · ${projectName(w.projectId)} · ${w.agent} · ${w.enabled ? "enabled" : "paused"} · ${w.source ? SOURCE_LABELS[w.source.kind] : "source not read yet"} · ${checked} · ${which}${titles}`);
    for (const t of w.tickets ?? []) {
      lines.push(`  #${t.id} · ${t.title} · ${t.type ? `${t.type} · ` : ""}${t.state} · ${asks(w, t) ? "asked about" : "not asked about"}${startedElsewhere(t) ? ` · started: ${startedElsewhere(t)}` : ""}${w.started?.[t.id] ? " · started here" : ""}`);
    }
  }
  for (const w of watches) {
    for (const t of w.waiting) lines.push(`waiting · ${w.id} · #${t.id} · ${t.title} · ${t.url}`);
  }
  return lines.length ? lines.join("\n") : "Nothing is set up yet.";
}
