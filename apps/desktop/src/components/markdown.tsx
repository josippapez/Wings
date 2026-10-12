import { openUrl } from "@tauri-apps/plugin-opener";

import { parseBlocks, parseInline } from "@/lib/markdown";

function Inline({ text }: { text: string }) {
  return parseInline(text).map((part, i) => {
    switch (part.kind) {
      case "code":
        return (
          <code key={i} className="rounded bg-white/[0.08] px-1 py-px font-mono text-[0.92em]">
            {part.text}
          </code>
        );
      case "strong":
        return <strong key={i}>{part.text}</strong>;
      case "em":
        return <em key={i}>{part.text}</em>;
      case "link":
        return (
          <a
            key={i}
            href={part.href}
            onClick={(e) => {
              e.preventDefault();
              void openUrl(part.href);
            }}
            className="text-working underline-offset-2 hover:underline"
          >
            {part.text}
          </a>
        );
      default:
        return part.text;
    }
  });
}

const headingClass = { 1: "text-[15px] font-semibold", 2: "text-[14px] font-semibold", 3: "text-[13px] font-semibold" } as const;

/** Renders a README as React elements: no HTML is ever parsed, and links open only when they are https. */
export function Markdown({ source }: { source: string }) {
  return (
    <div className="flex flex-col gap-2.5 text-[13px] leading-relaxed break-words">
      {parseBlocks(source).map((b, i) => {
        switch (b.kind) {
          case "heading": {
            // The dialog's own headings are h2 and h3, so a README's start below them.
            const Tag = `h${b.level + 3}` as "h4" | "h5" | "h6";
            return (
              <Tag key={i} className={headingClass[b.level]}>
                <Inline text={b.text} />
              </Tag>
            );
          }
          case "list": {
            const List = b.ordered ? "ol" : "ul";
            return (
              <List key={i} className={b.ordered ? "list-decimal pl-5" : "list-disc pl-5"}>
                {b.items.map((item, j) => (
                  <li key={j}>
                    <Inline text={item} />
                  </li>
                ))}
              </List>
            );
          }
          case "code":
            return (
              <pre key={i} className="overflow-x-auto rounded-lg bg-white/[0.06] p-3 font-mono text-[12px] leading-snug">
                {b.text}
              </pre>
            );
          case "quote":
            return (
              <blockquote key={i} className="border-l-2 border-hairline pl-3 text-muted-foreground">
                <Inline text={b.text} />
              </blockquote>
            );
          case "rule":
            return <hr key={i} className="border-hairline" />;
          default:
            return (
              <p key={i}>
                <Inline text={b.text} />
              </p>
            );
        }
      })}
    </div>
  );
}
