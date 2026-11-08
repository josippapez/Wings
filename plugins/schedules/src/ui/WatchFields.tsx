// The fields only an "assigned" entry has: what it tells the agent, which tickets it asks about, and where it starts them.

import { useEffect, useState } from "react";
import { Chip, Combobox, Field, FieldGroup, Notice, Row, Stack, TextInput, Textarea } from "wings-plugin-ui";
import { DEFAULT_MIN_FREE_GB, DEFAULT_PROMPT, stateOptions, type WatchDraft } from "../lib/assigned";
import type { WatchView } from "../lib/messages";
import { includeText, worktreeChange, type WorktreeChange, type WorktreeSetup } from "../lib/worktree-setup";

/**
 * What the "assigned" fields hold, from `watch` or a new entry's defaults, and the draft they make. While `active`, it
 * loads the branches and the worktree setting of `projectId`.
 */
export function useWatchFields(
  watch: WatchView | null,
  active: boolean,
  projectId: string,
  loadBranches?: (projectId: string) => Promise<string[]>,
  loadWorktreeSetup?: (projectId: string) => Promise<WorktreeSetup>,
) {
  const [template, setTemplate] = useState(watch?.prompt ?? DEFAULT_PROMPT);
  const [minFree, setMinFree] = useState(String(watch?.minFreeGb ?? DEFAULT_MIN_FREE_GB));
  const [states, setStates] = useState<string[] | undefined>(watch?.states);
  const [prefixes, setPrefixes] = useState((watch?.prefixes ?? []).join(", "));
  const [base, setBase] = useState(watch?.base ?? "");
  const [branches, setBranches] = useState<string[]>([]);
  useEffect(() => {
    if (!active || !projectId || !loadBranches) return;
    let live = true;
    loadBranches(projectId).then((b) => live && setBranches(b), () => live && setBranches([]));
    return () => void (live = false);
  }, [active, projectId, loadBranches]);
  // The project's own setting, loaded for the project picked; the fields start from it and only a change is saved.
  const [loaded, setLoaded] = useState<{ projectId: string; setup: WorktreeSetup } | null>(null);
  const [includeField, setIncludeField] = useState("");
  const [setupField, setSetupField] = useState("");
  useEffect(() => {
    setLoaded(null);
    if (!active || !projectId || !loadWorktreeSetup) return;
    let live = true;
    loadWorktreeSetup(projectId).then(
      (setup) => {
        if (!live) return;
        setLoaded({ projectId, setup });
        setIncludeField(includeText(setup));
        setSetupField(setup.setup ?? "");
      },
      () => {},
    );
    return () => void (live = false);
  }, [active, projectId, loadWorktreeSetup]);

  /** The entry with the `shared` fields, and the change to the worktree setting when there is one. */
  const draft = (shared: Pick<WatchDraft, "name" | "projectId" | "agent">): [WatchDraft, { projectId: string; change: WorktreeChange } | undefined] => {
    const change = loaded && worktreeChange(loaded.setup, includeField, setupField);
    return [
      // Another project has other states, so it picks its own at its first check.
      { ...shared, prompt: template, minFreeGb: Number(minFree), states: watch && watch.projectId !== shared.projectId ? undefined : states, prefixes: prefixes.split(",").map((p) => p.trim()), base },
      loaded && change ? { projectId: loaded.projectId, change } : undefined,
    ];
  };
  return { template, setTemplate, minFree, setMinFree, states, setStates, prefixes, setPrefixes, base, setBase, branches, loaded, includeField, setIncludeField, setupField, setSetupField, draft };
}

export function WatchFields({ watch, fields }: { watch: WatchView | null; fields: ReturnType<typeof useWatchFields> }) {
  const { template, setTemplate, minFree, setMinFree, states, setStates, prefixes, setPrefixes, base, setBase, branches, loaded, includeField, setIncludeField, setupField, setSetupField } = fields;
  const toggleState = (name: string) => setStates((states ?? []).includes(name) ? (states ?? []).filter((x) => x !== name) : [...(states ?? []), name]);
  const stateNames = watch ? stateOptions({ states, tickets: watch.tickets }) : [];
  return (
    <>
      <Field label="Prompt" hint="Sent to the agent as one line. {id}, {title} and {url} become the ticket's number, title and link.">
        <Textarea value={template} onChange={(e) => setTemplate(e.target.value)} rows={4} />
      </Field>
      <Field label="Keep free" hint="In GB. Wings won't make the worktree when the disk it goes on has less room than this.">
        <TextInput type="number" min={0} step={1} value={minFree} onChange={(e) => setMinFree(e.target.value)} required />
      </Field>
      <FieldGroup
        label="Ask about tickets in these states"
        hint={
          stateNames.length
            ? "Tickets in other states are listed but not asked about. A ticket is asked about once, and one that moves into these states later is asked about then. Leave all off to ask about every state."
            : "The states show up after the first check. Wings then picks the ones that mean nobody has started: Open on GitHub and GitLab, and New, To Do, Ready for Development and the like on Azure DevOps."
        }
      >
        {stateNames.length ? (
          <Row gap="s" wrap>
            {stateNames.map((name) => (
              <Chip key={name} pressed={(states ?? []).includes(name)} onClick={() => toggleState(name)}>
                {name}
              </Chip>
            ))}
          </Row>
        ) : null}
      </FieldGroup>
      <Field label="Start branches from" hint="Pick one of this project's branches or type one, like origin/develop. Empty starts from the repo's default branch.">
        <Combobox label="Start branches from" freeText value={base} onInput={setBase} onPick={(o) => setBase(o.value)} options={branches.map((b) => ({ value: b, label: b }))} maxLength={200} placeholder="The default branch" />
      </Field>
      <Field label="Only titles starting with" hint="Optional, separated by commas, like FE or BE. Other tickets are listed as for another project and not asked about. Empty asks about every title.">
        <TextInput value={prefixes} onChange={(e) => setPrefixes(e.target.value)} maxLength={200} placeholder="FE, BE" />
      </Field>
      <FieldGroup label="New worktrees" hint="The project's own settings, shared with Worktree setup in the sidebar: a change here changes it there too.">
        <Stack gap="m">
          <Field
            label="Copy into new worktrees"
            hint={`One pattern per line, .gitignore syntax: only files the repo ignores are copied, and a line starting with ! leaves files out. They are copied, not linked, so leave big folders out.${loaded?.setup.hasIncludeFile ? " This repo has a .worktreeinclude, so that file is used and this list is ignored." : ""}`}
          >
            <Textarea value={includeField} onChange={(e) => setIncludeField(e.target.value)} rows={6} disabled={!loaded} spellCheck={false} />
          </Field>
          <Field label="Setup command" hint="One shell line, like pnpm install. It runs in the new worktree before the agent starts, and the agent starts once it succeeds.">
            <TextInput value={setupField} onChange={(e) => setSetupField(e.target.value)} disabled={!loaded} spellCheck={false} placeholder="pnpm install" />
          </Field>
        </Stack>
      </FieldGroup>
      <Notice>Wings asks the project's Azure DevOps, GitHub or GitLab what is assigned to you every 5 minutes, using its origin remote. You are asked first: nothing starts until you press Start.</Notice>
    </>
  );
}
