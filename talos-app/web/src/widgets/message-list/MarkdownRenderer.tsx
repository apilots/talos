import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import { useMemo, type ComponentProps } from "react";

interface MarkdownRendererProps {
  className: string;
  text: string;
}

export function MarkdownRenderer({ className, text }: MarkdownRendererProps) {
  const markdownComponents = useMemo(
    () => ({
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
      <ReactMarkdown components={markdownComponents} rehypePlugins={[rehypeHighlight]} remarkPlugins={[remarkGfm]}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
