import { truncate } from "wings-plugin-sdk/format";

export const oneLine = (text: unknown, max = 90) => truncate(String(text ?? "").replace(/\s+/g, " ").trim(), max);

export const modelName = (model: string | null) => {
  const m = /^claude-([a-z]+)-(\d+)-(\d+)/.exec(model ?? "");
  return m ? `${m[1][0].toUpperCase()}${m[1].slice(1)} ${m[2]}.${m[3]}` : (model ?? "");
};
export const tokens = (n: number | null) => (n == null ? "—" : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
