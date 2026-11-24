import { useMemo } from "react";
import { marked } from "marked";

interface MarkdownViewProps {
  content: string;
  streaming?: boolean;
  className?: string;
}

function escapeRawHtml(md: string): string {
  // Neutralize raw HTML tags while preserving Markdown syntax and fenced code blocks
  const segments = md.split(/(```[\s\S]*?```|`[^`\n]+`)/g);
  return segments
    .map((seg, i) => {
      if (i % 2 === 1) return seg;
      return seg.replace(/</g, "&lt;").replace(/>/g, "&gt;");
    })
    .join("");
}

marked.setOptions({
  breaks: true,
  gfm: true,
});

export function MarkdownView({ content, streaming, className = "" }: MarkdownViewProps) {
  const html = useMemo(() => {
    if (!content) return "";
    const safe = escapeRawHtml(content);
    return marked.parse(safe, { async: false }) as string;
  }, [content]);

  return (
    <div
      className={`md-prose ${streaming ? "streaming-cursor" : ""} ${className}`}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
