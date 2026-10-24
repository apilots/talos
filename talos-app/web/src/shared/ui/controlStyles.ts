/** Theme-aware presentation shared by controls with existing selection/state owners. */
const selectionFocusClassName = "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/40";

export const selectionClasses = {
  selected: `bg-selected text-ink hover:bg-selected hover:text-ink focus-visible:bg-selected focus-visible:text-ink ${selectionFocusClassName}`,
  idle: `text-muted hover:bg-hover hover:text-ink ${selectionFocusClassName}`,
  border: "border-selectedBorder hover:border-selectedBorder",
} as const;

export const actionClasses = {
  primary: "bg-brand text-background enabled:hover:bg-actionHover disabled:bg-brandDim disabled:text-muted",
  secondary: "border border-border bg-card text-ink enabled:hover:border-selectedBorder enabled:hover:bg-hover disabled:bg-brandDim disabled:text-muted",
  ghost: "text-muted enabled:hover:bg-hover enabled:hover:text-ink disabled:text-muted/60",
  danger: "bg-danger/15 text-danger enabled:hover:bg-danger/25 disabled:opacity-50",
} as const;

export const toggleClasses = {
  on: "bg-brand text-background",
  off: "bg-hover text-muted",
} as const;
