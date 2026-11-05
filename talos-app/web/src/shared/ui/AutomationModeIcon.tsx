import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function IconFrame({ size = 15, children, ...props }: IconProps) {
  return <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.3} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false" {...props}>{children}</svg>;
}

/** The approved Goal/Loop geometry shared by entries, task rows and Input metadata. */
export function AutomationModeIcon({ mode, ...props }: IconProps & { mode: "goal" | "loop" }) {
  return <IconFrame {...props}>{mode === "goal" ? <>
    <circle cx="10" cy="10" r="6.5" /><circle cx="10" cy="10" r="3" /><path d="m10 10 6-6m-2 0h2v2" />
  </> : <path d="M16 7a6 6 0 0 0-10-2L3 8m0-4v4h4M4 13a6 6 0 0 0 10 2l3-3m0 4v-4h-4" />}</IconFrame>;
}

/** Compact task controls use the same paths as the approved composer sketch. */
export function AutomationActionIcon({ action, ...props }: IconProps & { action: "pause" | "resume" | "stop" | "clear" | "edit" }) {
  if (action === "clear") return <IconFrame size={14} viewBox="0 0 16 16" strokeLinecap="butt" strokeLinejoin="miter" {...props}><path d="M3 4.5h10M6 4V2.5h4V4M4.5 5.5l.5 8h6l.5-8M6.5 6.5v5M9.5 6.5v5" /></IconFrame>;
  return <IconFrame {...props}>
    {action === "pause" ? <path d="M7 5v10M13 5v10" /> : null}
    {action === "resume" ? <><circle cx="10" cy="10" r="7" /><path d="m8 6 5 4-5 4z" /></> : null}
    {action === "stop" ? <rect x="5" y="5" width="10" height="10" rx="1" /> : null}
    {action === "edit" ? <path d="M5 8V5h3m4 0h3v3M5 12v3h3m4 0h3v-3" /> : null}
  </IconFrame>;
}
