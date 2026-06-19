import { Layers3 } from "lucide-react";

export function PluginsEmptyState() {
  return (
    <div className="mt-10 rounded-[1.25rem] border border-dashed border-border/80 bg-card/55 px-6 py-12 text-center">
      <Layers3 className="mx-auto text-muted" size={28} />
      <div className="mt-4 text-[16px] font-semibold text-ink">No plugins installed</div>
      <div className="mx-auto mt-1 max-w-md text-sm leading-6 text-muted">
        Plugin creation and marketplace install are not wired to a backend API yet.
      </div>
    </div>
  );
}
