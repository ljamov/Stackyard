// Line icons for the workbench, drawn on a 24-unit grid in the text color.
// Sizes and stroke come from the CSS around them.

const PATHS = {
  explorer: "M4 5.5h6l2 2h8v11H4zM4 9.5h16",
  dashboard: "M4 4h7v7H4zM13 4h7v4h-7zM13 10h7v10h-7zM4 13h7v7H4z",
  sites: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9z",
  services: "M4 4h16v6H4zM4 14h16v6H4zM7.5 7h.01M7.5 17h.01M11 7h6M11 17h6",
  runtimes: "M8.5 7 3.5 12l5 5M15.5 7l5 5-5 5M13.5 5l-3 14",
  databases:
    "M12 3c4.4 0 8 1.3 8 3s-3.6 3-8 3-8-1.3-8-3 3.6-3 8-3zM4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
  scheduler: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3.5 2",
  images: "M3.5 4.5h17v15h-17zM3.5 16l5-5 4 4 3-3 5 5M15.5 9.5h.01",
  videos: "M3.5 5.5h12v13h-12zM15.5 10l5-3v10l-5-3",
  ai: "M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
  mcp: "M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4",
  logs: "M4 4.5h16v15H4zM7.5 9l3 3-3 3M12.5 15h4",
  settings: "M4 6.5h9M17 6.5h3M4 12h3M11 12h9M4 17.5h11M19 17.5h1M15 4.5v4M9 10v4M17 15.5v4",
  play: "M8 5.5v13l10.5-6.5z",
  stop: "M7 7h10v10H7z",
  close: "M7 7l10 10M17 7 7 17",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name }: { name: IconName }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={PATHS[name]} />
    </svg>
  );
}
