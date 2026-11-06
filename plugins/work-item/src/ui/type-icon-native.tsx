import { Dot } from "wings-plugin-ui";
import { typeStyle } from "../lib/type";

export function TypeIcon({ type }: { type: string; size?: number }) {
  return <Dot color={typeStyle(type).color} />;
}
