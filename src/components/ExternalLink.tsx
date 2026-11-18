import { api } from "../lib/api";

/** A web address shown in full that opens in the default browser when clicked. */
export function ExternalLink({ url, className }: { url: string; className?: string }) {
  const web = /^https?:\/\//i.test(url);
  if (!web) return <span className={className ?? "mono"}>{url}</span>;
  return (
    <a
      href={url}
      className={`external-link ${className ?? "mono"}`}
      title={url}
      onClick={(e) => {
        e.preventDefault();
        void api.openLink(url).catch(() => {});
      }}
    >
      {url}
    </a>
  );
}
