import type { PluginDocument } from "@talos/sdk-browser";

import { stripFrontmatter } from "@/features/plugins/lib/pluginDisplay";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";

interface PluginDocumentBodyProps {
  document: PluginDocument;
}

export function PluginDocumentBody({ document }: PluginDocumentBodyProps) {
  const contents = document.media_type === "markdown"
    ? stripFrontmatter(document.contents)
    : document.contents;
  if (document.media_type === "json") {
    return (
      <pre className="overflow-x-auto rounded-[0.8rem] border border-border bg-card px-4 py-3 text-xs leading-6 text-ink">
        {contents}
      </pre>
    );
  }
  return (
    <MarkdownRenderer
      className="markdown-body prose prose-sm max-w-none prose-headings:font-display prose-p:leading-[1.8]"
      text={contents}
    />
  );
}
