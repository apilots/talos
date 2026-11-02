import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { useMemo, type ComponentProps, type ReactNode } from "react";
import { FileImage } from "./FileImage";
import { fileReferenceId } from "@/shared/lib/open-resource";
import { ResourceLink } from "@/shared/ui/ResourceLink";

// Explicit File references use the authenticated client, never a public download URL.

interface MarkdownRendererProps {
  className: string;
  text: string;
  renderFileImage?: (fileId: string, label: string) => ReactNode;
  resolveImageUrl?: (url: string) => string;
  onOpenLink?: (url: string) => Promise<void>;
}

export function MarkdownRenderer({ className, text, renderFileImage, resolveImageUrl, onOpenLink }: MarkdownRendererProps) {
  const markdownComponents = useMemo(
    () => ({
      a: (props: ComponentProps<"a">) => <ResourceLink {...props} onOpenLink={onOpenLink} />,
      img({ src, alt, title }: ComponentProps<"img">) {
        const fileId = typeof src === "string" ? fileReferenceId(src) : undefined;
        return fileId ? renderFileImage ? renderFileImage(fileId, alt || fileId) : <FileImage fileId={fileId} label={alt || fileId} /> : <img src={typeof src === "string" && resolveImageUrl ? resolveImageUrl(src) : src || undefined} alt={alt ?? ""} title={title} />;
      },
      table(props: ComponentProps<"table">) {
        return (
          <div className="my-4 overflow-x-auto">
            <table {...props} />
          </div>
        );
      },
    }),
    [renderFileImage, resolveImageUrl, onOpenLink],
  );

  return (
    <div className={className}>
      <ReactMarkdown components={markdownComponents} rehypePlugins={[rehypeHighlight]} remarkPlugins={[remarkGfm]} urlTransform={(url, key) => fileReferenceId(url) || (key === "href" && /^file:\/\//i.test(url)) ? url : defaultUrlTransform(url)}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
