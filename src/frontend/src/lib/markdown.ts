import { escapeHtml } from "./utils";

/**
 * Minimal, dependency-free Markdown → HTML for the document preview pane.
 * Escapes all input first, then applies a safe subset (headings, emphasis,
 * inline code, fenced code, lists, blockquotes, links, hr, paragraphs).
 * Not a full CommonMark implementation — deliberately small and safe.
 */
export function renderMarkdown(src: string): string {
  const lines = escapeHtml(src ?? "").split("\n");
  const out: string[] = [];
  let inCode = false;
  let listType: "ul" | "ol" | null = null;

  function closeList() {
    if (listType) {
      out.push(`</${listType}>`);
      listType = null;
    }
  }

  function inline(text: string): string {
    return text
      .replace(/`([^`]+)`/g, '<code class="rounded bg-muted px-1 py-0.5 font-mono text-[0.85em]">$1</code>')
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/(^|[^*])\*([^*]+)\*/g, "$1<em>$2</em>")
      .replace(
        /\[([^\]]+)\]\((https?:[^)\s]+)\)/g,
        '<a href="$2" target="_blank" rel="noopener noreferrer" class="text-primary underline underline-offset-2">$1</a>'
      );
  }

  for (const raw of lines) {
    const line = raw;

    if (/^```/.test(line.trim())) {
      if (inCode) {
        out.push("</code></pre>");
        inCode = false;
      } else {
        closeList();
        out.push('<pre class="overflow-x-auto rounded-md border border-border bg-muted/60 p-3 text-xs"><code class="font-mono">');
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      out.push(line + "\n");
      continue;
    }

    if (line.trim() === "") {
      closeList();
      continue;
    }

    const h = line.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      closeList();
      const level = h[1].length;
      const sizes = ["text-2xl", "text-xl", "text-lg", "text-base", "text-sm", "text-sm"];
      out.push(
        `<h${level} class="mt-4 mb-2 font-semibold ${sizes[level - 1]} text-foreground">${inline(h[2])}</h${level}>`
      );
      continue;
    }

    if (/^(-{3,}|\*{3,})$/.test(line.trim())) {
      closeList();
      out.push('<hr class="my-4 border-border" />');
      continue;
    }

    if (/^>\s?/.test(line)) {
      closeList();
      out.push(
        `<blockquote class="my-2 border-l-2 border-border pl-3 text-muted-foreground">${inline(
          line.replace(/^>\s?/, "")
        )}</blockquote>`
      );
      continue;
    }

    const ol = line.match(/^\s*\d+\.\s+(.*)$/);
    const ul = line.match(/^\s*[-*]\s+(.*)$/);
    if (ol || ul) {
      const type = ol ? "ol" : "ul";
      if (listType !== type) {
        closeList();
        const cls = type === "ol" ? "list-decimal" : "list-disc";
        out.push(`<${type} class="my-2 ml-5 space-y-1 ${cls} text-foreground">`);
        listType = type;
      }
      out.push(`<li>${inline((ol ?? ul)![1])}</li>`);
      continue;
    }

    closeList();
    out.push(`<p class="my-2 leading-relaxed text-foreground">${inline(line)}</p>`);
  }

  if (inCode) out.push("</code></pre>");
  closeList();
  return out.join("");
}
