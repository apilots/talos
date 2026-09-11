import type { InputHTMLAttributes, ReactNode, TextareaHTMLAttributes } from "react";

export function FormSection({ children, title }: { children: ReactNode; title: string }) {
  return <div className="space-y-3 border-b border-border/45 px-5 py-4 last:border-b-0"><h3 className="text-sm font-semibold text-ink">{title}</h3>{children}</div>;
}
const control = "form-control-focus w-full rounded-[0.75rem] border border-border/75 bg-background/60 px-3 py-2 text-sm text-ink placeholder:text-muted/70 read-only:text-muted";
export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={control} type="text" {...props} />;
}
export function TextArea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className={`${control} min-h-24 font-mono`} spellCheck={false} {...props} />;
}

/** One key=value per line; values may contain equals signs. Never silently overwrite duplicates. */
export function parseEntries(text: string): Record<string, string> {
  const entries: Record<string, string> = Object.create(null) as Record<string, string>;
  for (const line of text.split(/\r?\n/).filter((line) => line.trim())) {
    const split = line.indexOf("=");
    const name = line.slice(0, split).trim();
    if (split < 1 || !name || Object.hasOwn(entries, name)) throw new Error("invalidEntries");
    entries[name] = line.slice(split + 1);
  }
  return entries;
}
export function formatEntries(values: Record<string, string | undefined> | undefined) {
  return Object.entries(values ?? {}).map(([key, value]) => `${key}=${value ?? ""}`).join("\n");
}
