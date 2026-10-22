// How long a scan has run, written as a clock that ticks once a second while Wings is in front.

import { useCallback, useState } from "react";
import { usePoll } from "wings-plugin-ui";
import { duration } from "./model";

/** Its own component so that the tick re-renders this text and not the page around it. */
export function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(() => Date.now());
  const tick = useCallback(() => setNow(Date.now()), []);
  usePoll(tick, 1000);
  return <>{duration(now - since)}</>;
}
