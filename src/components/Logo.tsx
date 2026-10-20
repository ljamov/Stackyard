// Stackyard mark: three offset stacked bars (logo concept A).
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <rect x="20" y="8" width="36" height="13" rx="3.5" fill="var(--logo-1)" />
      <rect x="8" y="25.5" width="36" height="13" rx="3.5" fill="var(--logo-2)" />
      <rect x="20" y="43" width="36" height="13" rx="3.5" fill="var(--logo-1)" />
    </svg>
  );
}
