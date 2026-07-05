export interface McpSourceView {
  id: string;
  count: string;
  name: string;
}

interface McpSourceListProps {
  sources: McpSourceView[];
  title: string;
}

export function McpSourceList({
  sources,
  title,
}: McpSourceListProps) {
  return (
    <section>
      <h3 className="mb-4 text-sm font-semibold text-ink">{title}</h3>
      <div className="overflow-hidden rounded-[1.05rem] border border-border/80 bg-card/45">
        {sources.map((source) => (
          <div
            className="grid min-h-16 grid-cols-[minmax(0,1fr)_max-content] items-center gap-4 border-b border-border/65 px-5 text-sm last:border-b-0"
            key={source.id}
          >
            <span className="min-w-0 truncate font-medium text-ink">{source.name}</span>
            <span className="text-muted">{source.count}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
