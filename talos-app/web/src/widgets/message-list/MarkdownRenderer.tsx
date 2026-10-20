import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { useMemo, type ComponentProps } from "react";
import { FileImage } from "./FileImage";

// Explicit File references use the authenticated client, never a public download URL.
function imageFileId(url: string) {
  return /^talos-file:\/\/([A-Za-z0-9_-]+)$/.exec(url)?.[1];
}

interface MarkdownRendererProps {
  className: string;
  text: string;
}

export function MarkdownRenderer({ className, text }: MarkdownRendererProps) {
  const markdownComponents = useMemo(
    () => ({
      img({ src, alt, title }: ComponentProps<"img">) {
        const fileId = typeof src === "string" ? imageFileId(src) : undefined;
        return fileId ? <FileImage fileId={fileId} label={alt || fileId} /> : <img src={src || undefined} alt={alt ?? ""} title={title} />;
      },
      table(props: ComponentProps<"table">) {
        return (
          <div className="my-4 overflow-x-auto">
            <table {...props} />
          </div>
        );
      },
    }),
    [],
  );

  return (
    <div className={className}>
      <ReactMarkdown components={markdownComponents} rehypePlugins={[rehypeHighlight]} remarkPlugins={[remarkGfm]} urlTransform={(url, key) => key === "src" && imageFileId(url) ? url : defaultUrlTransform(url)}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
