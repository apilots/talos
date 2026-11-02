import { cn } from "@/shared/lib/cn";

type TraceTabOption<T extends string> = {
  value: T;
  label: string;
  id: string;
  controls: string;
};

export function TraceTabs<T extends string>({
  tabs,
  value,
  onValueChange,
  label,
  className,
}: {
  tabs: ReadonlyArray<TraceTabOption<T>>;
  value: T;
  onValueChange: (value: T) => void;
  label: string;
  className?: string;
}) {
  return (
    <nav
      role="tablist"
      aria-label={label}
      className={cn("tracing-tabs", className)}
    >
      {tabs.map((tab, index) => (
        <button
          key={tab.value}
          type="button"
          id={tab.id}
          role="tab"
          aria-controls={tab.controls}
          aria-selected={value === tab.value}
          tabIndex={value === tab.value ? 0 : -1}
          className="tracing-tab"
          onClick={(event) => {
            onValueChange(tab.value);
            event.currentTarget.scrollIntoView({
              block: "nearest",
              inline: "nearest",
            });
          }}
          onKeyDown={(event) => {
            const nextIndex =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? tabs.length - 1
                  : event.key === "ArrowRight"
                    ? (index + 1) % tabs.length
                    : event.key === "ArrowLeft"
                      ? (index + tabs.length - 1) % tabs.length
                      : undefined;
            if (nextIndex === undefined) return;
            event.preventDefault();
            onValueChange(tabs[nextIndex].value);
            const next =
              event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>(
                '[role="tab"]',
              )[nextIndex];
            next?.focus();
            next?.scrollIntoView({ block: "nearest", inline: "nearest" });
          }}
        >
          {tab.label}
        </button>
      ))}
    </nav>
  );
}
