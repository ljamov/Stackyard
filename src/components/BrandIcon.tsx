import { useEffect, useState, type CSSProperties } from "react";
import { BRAND_PATHS } from "../lib/brands";
import { brandColor, onPaint, paintedColors } from "../lib/theme";
import { Icon, type IconName } from "./Icon";

/**
 * A service's or runtime's own logo in its brand color, on a tile tinted with
 * the same color. Ones without a logo here get a line icon on a plain tile.
 */
export function BrandIcon({ id, fallback = "services" }: { id: string; fallback?: IconName }) {
  const [colors, setColors] = useState(paintedColors);
  useEffect(() => onPaint(setColors), []);
  const path = BRAND_PATHS[id];
  const color = path && colors ? brandColor(id, colors["card.background"]) : null;
  if (!path || !color) {
    return (
      <span className="brand-icon plain" aria-hidden="true">
        <Icon name={fallback} />
      </span>
    );
  }
  return (
    <span className="brand-icon" style={{ "--brand": color } as CSSProperties} aria-hidden="true">
      <svg viewBox="0 0 24 24">
        <path d={path} />
      </svg>
    </span>
  );
}
