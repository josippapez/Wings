export const clip = (text: string, max: number) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);

export const basename = (path: unknown) => String(path ?? "").split("/").filter(Boolean).at(-1) ?? "";
export const oneLine = (text: unknown, max = 90) => {
  const line = String(text ?? "").replace(/\s+/g, " ").trim();
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
};

export const modelName = (model: string | null) => {
  const m = /^claude-([a-z]+)-(\d+)-(\d+)/.exec(model ?? "");
  return m ? `${m[1][0].toUpperCase()}${m[1].slice(1)} ${m[2]}.${m[3]}` : (model ?? "");
};
export const tokens = (n: number | null) => (n == null ? "—" : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
