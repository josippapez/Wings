// What the fixture's page and main say to each other, declared once the way a real plugin does it (see
// plugins/schedules/src/lib/messages.ts).
import { channel } from "wings-plugin-sdk/messages";

export const ch = channel<
  {
    /** Adds one to the count, and answers the new count. */
    increment: { request: void; response: number };
    /** Answers how long the text is: a way to send main a message too big for one broadcast. */
    echo: { request: string; response: number };
  },
  {},
  { count: { key: void; value: number } }
>();
