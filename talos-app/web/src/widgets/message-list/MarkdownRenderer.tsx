import { isRootFileLineCitation } from "@talos/sdk-core";
import ReactMarkdown, { defaultUrlTransform, type ExtraProps } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { createContext, memo, useContext, useMemo, type ComponentProps, type ReactNode } from "react";
import { FileImage } from "./FileImage";
import { cn } from "@/shared/lib/cn";
import { fileReferenceId, inlineCodeResourceReference } from "@/shared/lib/open-resource";
import { useAppStore } from "@/shared/store/app-store";
import { ResourceLink } from "@/shared/ui/ResourceLink";

// Explicit File references use the authenticated client, never a public download URL.

const PlainCodeContext = createContext(false);

interface MarkdownRendererProps {
  className: string;
  text: string;
  renderFileImage?: (fileId: string, label: string) => ReactNode;
  resolveImageUrl?: (url: string) => string;
  onOpenLink?: (url: string) => Promise<void>;
  interactiveCodeReferences?: boolean;
}

function InteractiveInlineCode({
  allowSessionFiles,
  children,
  className,
  node: _node,
  onOpenLink,
  ...props
}: ComponentProps<"code"> & ExtraProps & { allowSessionFiles: boolean; onOpenLink?: (url: string) => Promise<void> }) {
  void _node;
  const plainCode = useContext(PlainCodeContext);
  const raw = !plainCode && typeof children === "string" ? children : null;
  const target = raw ? inlineCodeResourceReference(raw, allowSessionFiles) : null;
  if (!target) {
    return <code {...props} className={className}>{children}</code>;
  }
  return (
    <ResourceLink
      href={target}
      onOpenLink={onOpenLink}
      className="rounded-[0.45rem] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/40"
    >
      <code
        {...props}
        className={cn(
          className,
          "cursor-pointer underline decoration-border underline-offset-2 transition-colors hover:border-brand/40 hover:decoration-ink/60",
        )}
      >
        {children}
      </code>
    </ResourceLink>
  );
}

export const MarkdownRenderer = memo(function MarkdownRenderer({
  className,
  text,
  renderFileImage,
  resolveImageUrl,
  onOpenLink,
  interactiveCodeReferences = false,
}: MarkdownRendererProps) {
  const hasSessionContext = useAppStore(
    (state) => interactiveCodeReferences && !onOpenLink && state.workspaceOwner?.context.kind === "session",
  );
  const allowSessionFiles = Boolean(onOpenLink) || hasSessionContext;

  const markdownComponents = useMemo(
    () => ({
      a: ({ node: _node, ...props }: ComponentProps<"a"> & ExtraProps) => {
        void _node;
        const link = <ResourceLink {...props} onOpenLink={onOpenLink} />;
        return interactiveCodeReferences ? <PlainCodeContext.Provider value={true}>{link}</PlainCodeContext.Provider> : link;
      },
      ...(interactiveCodeReferences
        ? {
            pre: ({ node: _node, ...props }: ComponentProps<"pre"> & ExtraProps) => {
              void _node;
              return (
                <PlainCodeContext.Provider value={true}>
                  <pre {...props} />
                </PlainCodeContext.Provider>
              );
            },
            code: (props: ComponentProps<"code"> & ExtraProps) => (
              <InteractiveInlineCode {...props} allowSessionFiles={allowSessionFiles} onOpenLink={onOpenLink} />
            ),
          }
        : {}),
      img({ src, alt, title }: ComponentProps<"img">) {
        const fileId = typeof src === "string" ? fileReferenceId(src) : undefined;
        return fileId ? renderFileImage ? renderFileImage(fileId, alt || fileId) : <FileImage fileId={fileId} label={alt || fileId} /> : <img src={typeof src === "string" && resolveImageUrl ? resolveImageUrl(src) : src || undefined} alt={alt ?? ""} title={title} />;
      },
      table({ node: _node, ...props }: ComponentProps<"table"> & ExtraProps) {
        void _node;
        return (
          <div className="my-4 overflow-x-auto">
            <table {...props} />
          </div>
        );
      },
    }),
    [renderFileImage, resolveImageUrl, onOpenLink, interactiveCodeReferences, allowSessionFiles],
  );

  return (
    <div className={className}>
      <ReactMarkdown
        components={markdownComponents}
        rehypePlugins={[rehypeHighlight]}
        remarkPlugins={[remarkGfm]}
        urlTransform={(url, key) =>
          fileReferenceId(url) || (key === "href" && (/^file:\/\//i.test(url) || isRootFileLineCitation(url)))
            ? url
            : defaultUrlTransform(url)
        }
      >
        {text}
      </ReactMarkdown>
    </div>
  );
});
