// The PR tracker's page in Wings Settings: how often to check for new pull requests, and the tokens for the hosts that
// have no CLI to sign in with. A token goes to the system keychain and can be sent by `wings.fetch` but never read back,
// so this page only shows whether one is saved.

import { useEffect, useState } from "react";
import { useSetting } from "wings-plugin-sdk/react";
import { Button, Checkbox, Confirm, Heading, Row, Select, Stack, Text, TextInput, useAction } from "wings-plugin-ui";
import { mentionsOn } from "../lib/inbox";
import { ch } from "../lib/messages";
import { WATCH_MINUTES, watchMinutes, watchOf, type WatchMinutes } from "../lib/watch";

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

const WATCH_OPTIONS = WATCH_MINUTES.map((m) => ({ value: String(m) as `${WatchMinutes}`, label: m ? `Every ${m} minutes` : "Off" }));

function WatchField() {
  const [minutes, setMinutes] = useSetting(watchMinutes);

  const change = (value: string) => void setMinutes(watchOf(Number(value)) ?? 0);

  return (
    <Stack gap="s">
      <Heading>New pull requests</Heading>
      <Text tone="muted">Get a notification when someone opens a pull request in one of your projects. While Wings is in front it checks every minute. In the background it checks this often.</Text>
      {minutes === null ? null : <Select label="Check in the background" options={WATCH_OPTIONS} value={String(minutes) as `${WatchMinutes}`} onChange={change} />}
    </Stack>
  );
}

function MentionsField() {
  const [on, setOn] = useSetting(mentionsOn);

  return (
    <Stack gap="s">
      <Heading>Comments for you</Heading>
      <Text tone="muted">Get a notification when someone mentions you, or replies in a thread you wrote in, on any pull request in your projects. It only checks while Wings is in the background, as often as above, or every 5 minutes when that is off.</Text>
      {on === null ? null : <Checkbox checked={on} onChange={(checked) => void setOn(checked)}>Notify me about mentions and replies</Checkbox>}
    </Stack>
  );
}

export default function PrTrackerSettings() {
  return (
    <Stack gap="l">
      <WatchField />
      <MentionsField />
      <Heading>Pull request hosts</Heading>
      <Text tone="muted">GitHub (including GitHub Enterprise), GitLab and Azure DevOps use the gh, glab and az sign-in. Bitbucket Cloud and Codeberg use a token saved here.</Text>
      {HOSTS.map((h) => <TokenField key={h.secret} host={h} />)}
    </Stack>
  );
}

