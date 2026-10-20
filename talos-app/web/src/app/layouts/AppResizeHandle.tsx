import { useEffect, useRef } from "react";

/** Shared by the two real pane boundaries; no document listeners or layout measurement. */
export function AppResizeHandle({ label, controls, value, min, max, direction = 1, onResize, onDragging, className }: {
  label: string; controls: string; value: number; min: number; max: number; direction?: 1 | -1;
  onResize: (value: number) => void; onDragging: (dragging: boolean) => void; className: string;
}) {
  const drag = useRef<{ id: number; x: number; value: number; target: HTMLDivElement } | null>(null);
  const latest = useRef({ onDragging }); latest.current = { onDragging };
  function end() {
    const current = drag.current;
    if (!current) return;
    drag.current = null;
    if (current.target.hasPointerCapture(current.id)) current.target.releasePointerCapture(current.id);
    latest.current.onDragging(false);
  }
  useEffect(() => () => {
    const current = drag.current;
    if (!current) return;
    drag.current = null;
    if (current.target.hasPointerCapture(current.id)) current.target.releasePointerCapture(current.id);
    latest.current.onDragging(false);
  }, []);
  const resize = (next: number) => onResize(Math.round(Math.min(max, Math.max(min, next))));
  return <div role="separator" aria-label={label} aria-orientation="vertical" aria-controls={controls} aria-valuemin={Math.round(min)} aria-valuemax={Math.round(max)} aria-valuenow={Math.round(value)} tabIndex={0}
    className={`absolute z-20 w-2 cursor-col-resize touch-none select-none outline-none focus-visible:bg-brand/15 ${className}`}
    onPointerDown={event => {
      if (event.button !== 0 || drag.current) return;
      event.preventDefault();event.currentTarget.focus();event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { id: event.pointerId, x: event.clientX, value, target: event.currentTarget };
      onDragging(true);
    }} onPointerMove={event => {
      const current = drag.current;
      if (current?.id === event.pointerId) resize(current.value + (event.clientX - current.x) * direction);
    }} onPointerUp={event => { if (drag.current?.id === event.pointerId) end(); }} onPointerCancel={event => { if (drag.current?.id === event.pointerId) end(); }} onLostPointerCapture={event => { if (drag.current?.id === event.pointerId) end(); }}
    onKeyDown={event => {
      const step = event.shiftKey ? 64 : 16;
      const next = event.key === "Home" ? min : event.key === "End" ? max
        : event.key === "ArrowLeft" ? value - step * direction : event.key === "ArrowRight" ? value + step * direction : null;
      if (next !== null) { event.preventDefault();resize(next); }
    }} />;
}
