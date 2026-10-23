import { useLayoutEffect, useRef, useState, type FocusEvent, type PointerEvent } from "react";
import { createPortal } from "react-dom";
import { Icon, type IconName } from "./Icon";

/** Where the tooltip goes: under the button, or over it near the bottom of the window. */
type Spot = { x: number; y: number; above: boolean };

/**
 * A square button with only an icon. What it does shows in a tooltip while
 * the pointer is over it or it has keyboard focus, and is its accessible name.
 * The tooltip sits on top of the page, so table and card edges never cut it.
 */
export function IconButton({
  icon,
  label,
  onClick,
  disabled = false,
  danger = false,
  busy = false,
}: {
  icon: IconName;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  /** Working on it: the button waits and says so. */
  busy?: boolean;
}) {
  const [spot, setSpot] = useState<Spot | null>(null);
  const tip = useRef<HTMLSpanElement>(null);

  const show = (el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    const above = r.bottom + 40 > window.innerHeight;
    setSpot({ x: r.left + r.width / 2, y: above ? r.top - 6 : r.bottom + 6, above });
  };
  const hide = () => setSpot(null);

  // Keep the tooltip inside the window at its sides.
  useLayoutEffect(() => {
    const el = tip.current;
    if (!el || !spot) return;
    const r = el.getBoundingClientRect();
    const shift = Math.max(8 - r.left, 0) - Math.max(r.right - (window.innerWidth - 8), 0);
    if (shift) el.style.marginLeft = `${shift}px`;
  }, [spot]);

  useLayoutEffect(() => {
    if (!spot) return;
    window.addEventListener("scroll", hide, true);
    return () => window.removeEventListener("scroll", hide, true);
  }, [spot]);

  return (
    <>
      <button
        type="button"
        className={danger ? "icon-action danger" : "icon-action"}
        aria-label={label}
        aria-busy={busy || undefined}
        disabled={disabled || busy}
        onPointerEnter={(e: PointerEvent<HTMLButtonElement>) => e.pointerType === "mouse" && show(e.currentTarget)}
        onPointerLeave={hide}
        onFocus={(e: FocusEvent<HTMLButtonElement>) => e.currentTarget.matches(":focus-visible") && show(e.currentTarget)}
        onBlur={hide}
        onClick={onClick}
      >
        <Icon name={icon} />
      </button>
      {spot &&
        createPortal(
          <span
            ref={tip}
            className={spot.above ? "hover-tip above" : "hover-tip"}
            style={{ left: spot.x, top: spot.y }}
            aria-hidden="true"
          >
            {label}
          </span>,
          document.body,
        )}
    </>
  );
}
