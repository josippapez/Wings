// The PR tracker's page in Wings Settings: how often to check for new pull requests, and the tokens for the hosts that
// have no CLI to sign in with. A token goes to the system keychain and can be sent by `wings.fetch` but never read back,
// so this page only shows whether one is saved.

import { useEffect, useState } from "react";
import { Button, Heading, Row, Select, Stack, Text, TextInput } from "wings-plugin-ui";
import { WATCH_KEY, WATCH_MINUTES, watchOf, type WatchMinutes } from "../lib/watch";

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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void wings.secrets.has(host.secret).then(setSaved);
  }, [host.secret]);

  const change = async (save: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await (save ? wings.secrets.set(host.secret, value.trim()) : wings.secrets.delete(host.secret));
      setSaved(save);
      setValue("");
      // The main script asks again with the new token.
      wings.broadcast({ settingsChanged: true });
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Stack gap="s">
      <Heading level={3}>{host.name}</Heading>
      <Text tone="muted">{host.help}</Text>
      <Text>{saved === null ? "" : saved ? "A token is saved." : "No token saved."}</Text>
      <Row gap="s">
        <TextInput label={`${host.name} token`} type="password" autoComplete="off" value={value} onChange={(e) => setValue(e.target.value)} />
        <Button size="sm" busy={busy} disabled={!value.trim()} onClick={() => void change(true)}>Save</Button>
        {saved ? <Button size="sm" variant="ghost" busy={busy} onClick={() => void change(false)}>Remove</Button> : null}
      </Row>
      {error ? <Text tone="danger" role="alert">{error}</Text> : null}
    </Stack>
  );
}

const WATCH_OPTIONS = WATCH_MINUTES.map((m) => ({ value: String(m) as `${WatchMinutes}`, label: m ? `Every ${m} minutes` : "Off" }));

function WatchField() {
  const [minutes, setMinutes] = useState<WatchMinutes | null>(null);

  useEffect(() => {
    void wings.storage.get(WATCH_KEY).then((saved) => setMinutes(watchOf(saved)));
  }, []);

  const change = async (value: string) => {
    const next = watchOf(Number(value));
    setMinutes(next);
    await wings.storage.set(WATCH_KEY, next);
    wings.broadcast({ watchChanged: true });
  };

  return (
    <Stack gap="s">
      <Heading>New pull requests</Heading>
      <Text tone="muted">Get a notification when someone opens a pull request in one of your projects. While Wings is in front it checks every minute. In the background it checks this often.</Text>
      {minutes === null ? null : <Select label="Check in the background" options={WATCH_OPTIONS} value={String(minutes) as `${WatchMinutes}`} onChange={(v) => void change(v)} />}
    </Stack>
  );
}

export default function PrTrackerSettings() {
  return (
    <Stack gap="l">
      <WatchField />
      <Heading>Pull request hosts</Heading>
      <Text tone="muted">GitHub (including GitHub Enterprise), GitLab and Azure DevOps use the gh, glab and az sign-in. Bitbucket Cloud and Codeberg use a token saved here.</Text>
      {HOSTS.map((h) => <TokenField key={h.secret} host={h} />)}
    </Stack>
  );
}

