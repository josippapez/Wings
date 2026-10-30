// The PR tracker's page in Wings Settings: what the sidebar lists, what you're notified about and how often to check, and
// the tokens for the hosts that have no CLI to sign in with. A token goes to the system keychain and can be sent by `wings.fetch` but never read back,
// so this page only shows whether one is saved.

import { useEffect, useState } from "react";
import { useSetting } from "wings-plugin-sdk/react";
import { Button, Checkbox, Confirm, Field, Heading, Row, Select, Stack, Text, TextInput, useAction } from "wings-plugin-ui";
import { mentionsOn } from "../lib/inbox";
import { ch } from "../lib/messages";
import { tellAgents } from "../lib/news";
import { NOTIFY, SIDEBAR, WATCH_MINUTES, notifyPulls, sidebarPulls, watchMinutes, watchOf, type Notify, type Sidebar, type WatchMinutes } from "../lib/watch";

export const page = { place: "settings" };

interface Host {
  secret: string;
  name: string;
  help: string;
}

const HOSTS: Host[] = [
  {
    secret: "bitbucket",
    name: "Bitbucket Cloud",
    help: "In your Atlassian account, open Security, Create and manage API tokens, Create API token with scopes, and pick Bitbucket. Give it the read:pullrequest:bitbucket and read:user:bitbucket scopes.",
  },
  {
    secret: "codeberg",
    name: "Codeberg",
    help: "Create an access token in your Codeberg account settings with the read:repository, read:issue and read:user scopes.",
  },
];

function TokenField({ host }: { host: Host }) {
  const [saved, setSaved] = useState<boolean | null>(null);
  const [value, setValue] = useState("");
  const { run, busy, error } = useAction();
  const [removing, setRemoving] = useState(false);

  useEffect(() => {
    void wings.secrets.has(host.secret).then(setSaved);
  }, [host.secret]);

  const change = (save: boolean) =>
    run(async () => {
      await (save ? wings.secrets.set(host.secret, value.trim()) : wings.secrets.delete(host.secret));
      setSaved(save);
      setValue("");
      // The main script asks again with the new token.
      await ch.tell("tokens-changed");
    });

  return (
    <Stack gap="s">
      <Heading level={3}>{host.name}</Heading>
      <Text tone="muted">{host.help}</Text>
      <Text>{saved === null ? "" : saved ? "A token is saved." : "No token saved."}</Text>
      <Row gap="s">
        <TextInput label={`${host.name} token`} type="password" autoComplete="off" value={value} onChange={(e) => setValue(e.target.value)} />
        <Button size="sm" busy={busy} disabled={!value.trim()} onClick={() => void change(true)}>Save</Button>
        {saved ? <Button size="sm" variant="ghost" busy={busy} onClick={() => setRemoving(true)}>Remove</Button> : null}
      </Row>
      {removing ? (
        <Confirm
          title={`Remove the ${host.name} token?`}
          cancelLabel="Keep it"
          onCancel={() => setRemoving(false)}
          actions={[{ id: "remove", label: "Remove", onConfirm: () => change(false).then((ok) => ok && setRemoving(false)) }]}
        >
          <Text size="small">{host.name} pull requests stop showing until you save a token again.</Text>
        </Confirm>
      ) : null}
      {error ? <Text tone="danger" role="alert">{error}</Text> : null}
    </Stack>
  );
}

const SIDEBAR_LABELS: Record<Sidebar, string> = { mine: "Yours and review requests", all: "Every open pull request" };
const SIDEBAR_OPTIONS = SIDEBAR.map((s) => ({ value: s, label: SIDEBAR_LABELS[s] }));

function SidebarField() {
  const [shown, setShown] = useSetting(sidebarPulls);

  return (
    <Stack gap="s">
      <Heading>Sidebar</Heading>
      <Text tone="muted">The pull requests listed under each project. Yours come first, then the ones waiting for your review, then everyone else's.</Text>
      {shown === null ? null : (
        <Field label="Sidebar lists">
          <Select options={SIDEBAR_OPTIONS} value={shown} onChange={(value) => void setShown(value)} />
        </Field>
      )}
    </Stack>
  );
}

const NOTIFY_LABELS: Record<Notify, string> = { off: "Off", review: "Only when I'm asked to review", all: "Every new pull request" };
const NOTIFY_OPTIONS = NOTIFY.map((n) => ({ value: n, label: NOTIFY_LABELS[n] }));
const WATCH_OPTIONS = WATCH_MINUTES.map((m) => ({ value: String(m) as `${WatchMinutes}`, label: `Every ${m} minutes` }));

function WatchField() {
  const [notify, setNotify] = useSetting(notifyPulls);
  const [minutes, setMinutes] = useSetting(watchMinutes);

  return (
    <Stack gap="s">
      <Heading>New pull requests</Heading>
      <Text tone="muted">Get a notification when someone opens a pull request in one of your projects, or only when one asks for your review, including one you're added to later. While Wings is in front it checks every minute. In the background it checks as often as you pick here.</Text>
      {notify === null ? null : (
        <Field label="Notify me about">
          <Select options={NOTIFY_OPTIONS} value={notify} onChange={(value) => void setNotify(value)} />
        </Field>
      )}
      {minutes === null ? null : (
        <Field label="Check in the background">
          <Select options={WATCH_OPTIONS} value={String(minutes) as `${WatchMinutes}`} onChange={(value) => void setMinutes(watchOf(Number(value)) ?? 5)} />
        </Field>
      )}
    </Stack>
  );
}

function MentionsField() {
  const [on, setOn] = useSetting(mentionsOn);

  return (
    <Stack gap="s">
      <Heading>Comments for you</Heading>
      <Text tone="muted">Get a notification when someone mentions you, or replies in a thread you wrote in, on any pull request in your projects. It only checks while Wings is in the background, as often as above.</Text>
      {on === null ? null : <Checkbox checked={on} onChange={(checked) => void setOn(checked)}>Notify me about mentions and replies</Checkbox>}
    </Stack>
  );
}

function AgentsField() {
  const [on, setOn] = useSetting(tellAgents);

  return (
    <Stack gap="s">
      <Heading>Agents</Heading>
      <Text tone="muted">When a coding agent works on a pull request's branch, send it a message when the pull request's checks fail, someone else comments on it or it gets merge conflicts. It checks those pull requests every 2 minutes, in the background too, and sends at most 5 messages an hour for each. Wings presses Enter for you only while "Let agents message each other" is on in Settings, Agents.</Text>
      {on === null ? null : <Checkbox checked={on} onChange={(checked) => void setOn(checked)}>Tell the agent on a PR's branch about failed checks and new review comments</Checkbox>}
    </Stack>
  );
}

export default function PrTrackerSettings() {
  return (
    <Stack gap="l">
      <SidebarField />
      <WatchField />
      <MentionsField />
      <AgentsField />
      <Heading>Pull request hosts</Heading>
      <Text tone="muted">GitHub (including GitHub Enterprise), GitLab and Azure DevOps use the gh, glab and az sign-in. Bitbucket Cloud and Codeberg use a token saved here.</Text>
      {HOSTS.map((h) => <TokenField key={h.secret} host={h} />)}
    </Stack>
  );
}

