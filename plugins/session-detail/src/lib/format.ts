import { truncate } from "wings-plugin-sdk/format";

export const oneLine = (text: unknown, max = 90) => truncate(String(text ?? "").replace(/\s+/g, " ").trim(), max);

export const modelName = (model: string | null) => {
  const m = /^claude-([a-z]+)-(\d+)-(\d+)/.exec(model ?? "");
  return m ? `${m[1][0].toUpperCase()}${m[1].slice(1)} ${m[2]}.${m[3]}` : (model ?? "");
};
