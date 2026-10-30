import { useHighlightedCode } from "@/hooks/use-highlighted-code";
import { CodeBlock, CodeBlockActions, CodeBlockContent, CodeBlockCopyButton, CodeBlockLine } from "@/components/ui/code-block";
import { cn } from "@/lib/utils";

/**
 * Code the user reads or copies: highlighted by `language` (plain when it is empty or not one the highlighter knows)
 * with a copy button on hover. The highlighter and each language load on first use, and a block re-tokenizes only
 * when its code or language changes.
 */
export function CodeSnippet({ code, language = "", wrap = false, className, ...props }: { code: string; language?: string; wrap?: boolean } & Omit<React.ComponentProps<typeof CodeBlock>, "children">) {
  const lines = useHighlightedCode(code, language);
  return (
    <CodeBlock size="sm" className={cn("border-0 bg-muted dark:bg-muted", className)} {...props}>
      <CodeBlockActions>
        <CodeBlockCopyButton value={code} />
      </CodeBlockActions>
      <CodeBlockContent wrap={wrap} className={cn(wrap && "break-all")}>
        {lines.map((line, i) => (
          <CodeBlockLine key={i}>{line}</CodeBlockLine>
        ))}
      </CodeBlockContent>
    </CodeBlock>
  );
}
