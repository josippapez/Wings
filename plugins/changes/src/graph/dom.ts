type Child = Node | string | null | false | undefined;
type Props = Record<string, string | boolean | null | undefined | EventListener>;

/** Strings become text, never HTML. */
export function h(tag: string, props: Props = {}, ...children: Child[]) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key.startsWith("on")) el.addEventListener(key.slice(2), value as EventListener);
    else if (key === "class") el.className = value as string;
    else if (value === true) el.setAttribute(key, "");
    else if (value !== false && value != null) el.setAttribute(key, String(value));
  }
  for (const child of children) if (child) el.append(child);
  return el;
}
