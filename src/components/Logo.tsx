// Stackyard mark: three stacked bars stepping down from right to left, like an S.
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <rect x="26" y="8" width="34" height="13" rx="3.5" fill="var(--logo-1)" />
      <rect x="22" y="25.5" width="20" height="13" rx="3.5" fill="var(--logo-2)" />
      <rect x="4" y="43" width="34" height="13" rx="3.5" fill="var(--logo-1)" />
    </svg>
  );
}
