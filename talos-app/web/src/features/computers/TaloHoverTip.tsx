import { cloneElement, isValidElement, useEffect, useId, useLayoutEffect, useRef, useState, type ReactElement } from "react";
import { createPortal } from "react-dom";

type TipPosition = { top: number; left: number; ready: boolean };

/** Minimalist portal tooltip that floats clear of overflow-clipped scroll containers. */
export function TaloHoverTip({
  content,
  children,
}: {
  content: string;
  children: ReactElement<Record<string, unknown>>;
}) {
  const tipId = useId();
  const [open, setOpen] = useState(false);
  const anchorElRef = useRef<HTMLElement | null>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<TipPosition>({ top: 0, left: 0, ready: false });

  useLayoutEffect(() => {
    if (!open || !anchorElRef.current || !tipRef.current) {
      setPos((prev) => (prev.ready ? { ...prev, ready: false } : prev));
      return;
    }
    const anchor = anchorElRef.current.getBoundingClientRect();
    const tip = tipRef.current.getBoundingClientRect();
    const gap = 8;
    const fitsLeft = anchor.left - gap - tip.width >= 8;
    const rawLeft = fitsLeft
      ? anchor.left - gap - tip.width
      : Math.max(8, Math.min(window.innerWidth - tip.width - 8, anchor.right - tip.width));
    const rawTop = fitsLeft
      ? anchor.top + (anchor.height - tip.height) / 2
      : anchor.top >= tip.height + gap + 8
        ? anchor.top - tip.height - gap
        : anchor.bottom + gap;
    const top = Math.max(8, Math.min(window.innerHeight - tip.height - 8, rawTop));
    setPos({ top, left: rawLeft, ready: true });
  }, [open, content]);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  if (!isValidElement(children)) return children;

  const childProps = children.props;
  const existingDescribedBy = typeof childProps["aria-describedby"] === "string" ? childProps["aria-describedby"] : undefined;
  const describedBy = open ? [existingDescribedBy, tipId].filter(Boolean).join(" ") : existingDescribedBy;

  const trigger = cloneElement(children, {
    "aria-describedby": describedBy,
    onMouseEnter: (event: MouseEvent) => {
      anchorElRef.current = event.currentTarget as HTMLElement;
      setOpen(true);
      (childProps.onMouseEnter as ((e: MouseEvent) => void) | undefined)?.(event);
    },
    onMouseLeave: (event: MouseEvent) => {
      setOpen(false);
      (childProps.onMouseLeave as ((e: MouseEvent) => void) | undefined)?.(event);
    },
    onFocus: (event: FocusEvent) => {
      anchorElRef.current = event.currentTarget as HTMLElement;
      setOpen(true);
      (childProps.onFocus as ((e: FocusEvent) => void) | undefined)?.(event);
    },
    onBlur: (event: FocusEvent) => {
      setOpen(false);
      (childProps.onBlur as ((e: FocusEvent) => void) | undefined)?.(event);
    },
  });

  return (
    <>
      {trigger}
      {open && typeof document !== "undefined"
        ? createPortal(
            <div
              ref={tipRef}
              id={tipId}
              role="tooltip"
              style={{ top: pos.top, left: pos.left, visibility: pos.ready ? "visible" : "hidden" }}
              className="pointer-events-none fixed z-[60] max-w-[230px] rounded-lg bg-ink px-2.5 py-1.5 text-[11px] font-medium leading-4 text-background shadow-md"
            >
              {content}
            </div>,
            document.body,
          )
        : null}
    </>
  );
}
