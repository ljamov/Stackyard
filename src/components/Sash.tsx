import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

/**
 * The line between two parts of the workbench. Drag it, or focus it and use
 * the arrow keys, to resize the part it belongs to.
 */
export function Sash({
  orientation,
  value,
  min,
  max,
  onChange,
  label,
  invert = false,
}: {
  /** "vertical" sits between side-by-side parts and resizes their width. */
  orientation: "vertical" | "horizontal";
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
  label: string;
  /** For a part after the line (the panel below it): dragging toward it makes it smaller. */
  invert?: boolean;
}) {
  const start = useRef<{ at: number; value: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const pos = (e: PointerEvent) => (orientation === "vertical" ? e.clientX : e.clientY);
  const set = (v: number) => onChange(Math.min(max, Math.max(min, Math.round(v))));

  function down(e: PointerEvent<HTMLDivElement>) {
    e.currentTarget.setPointerCapture(e.pointerId);
    start.current = { at: pos(e), value };
    setDragging(true);
  }
  function move(e: PointerEvent) {
    if (!start.current) return;
    const delta = pos(e) - start.current.at;
    set(start.current.value + (invert ? -delta : delta));
  }
  function up() {
    start.current = null;
    setDragging(false);
  }
  function key(e: KeyboardEvent) {
    const grow = orientation === "vertical" ? "ArrowRight" : "ArrowDown";
    const shrink = orientation === "vertical" ? "ArrowLeft" : "ArrowUp";
    if (e.key !== grow && e.key !== shrink) return;
    e.preventDefault();
    set(value + ((e.key === grow) !== invert ? 16 : -16));
  }

  return (
    <div
      className={`sash ${orientation}${dragging ? " dragging" : ""}`}
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onKeyDown={key}
    />
  );
}
