import { useEffect, useState } from "react";
import { api, type Site } from "../lib/api";
import { frameworkBrand } from "../lib/brands";
import { useT } from "../lib/i18n";
import { BrandIcon } from "./BrandIcon";
import "./site-views.css";

/** Pictures already fetched this session, by domain; null = the site has none. */
const pictures = new Map<string, string | null>();
const listeners = new Set<(domain: string) => void>();
/** One fetch per domain at a time, shared by every picture of that site. */
const pending = new Map<string, Promise<string | null>>();

function load(domain: string, refresh: boolean) {
  const running = pending.get(domain);
  if (running && !refresh) return running;
  const next = api
    .siteThumbnail(domain, refresh)
    .catch(() => pictures.get(domain) ?? null)
    .then((url) => {
      pictures.set(domain, url);
      pending.delete(domain);
      listeners.forEach((l) => l(domain));
      return url;
    });
  pending.set(domain, next);
  return next;
}

/**
 * A picture of the site's home page, like the site cards in Local or
 * WordPress Studio. Until there is one (or without a browser to take it)
 * the site's framework logo stands in.
 */
export function SiteThumb({ site, large = false }: { site: Site; large?: boolean }) {
  const t = useT();
  const [url, setUrl] = useState(() => pictures.get(site.domain) ?? null);
  const [taking, setTaking] = useState(false);

  useEffect(() => {
    const domain = site.domain;
    const changed = (d: string) => d === domain && setUrl(pictures.get(domain) ?? null);
    listeners.add(changed);
    if (!pictures.has(domain)) load(domain, false);
    else setUrl(pictures.get(domain) ?? null);
    return () => void listeners.delete(changed);
  }, [site.domain]);

  async function retake() {
    setTaking(true);
    await load(site.domain, true);
    setTaking(false);
  }

  return (
    <div className={large ? "site-thumb large" : "site-thumb"}>
      {url ? (
        <img src={url} alt={t("The home page of {site}", { site: site.domain })} loading="lazy" draggable={false} />
      ) : (
        <span className="site-thumb-empty" aria-hidden="true">
          <BrandIcon id={frameworkBrand(site.framework) ?? ""} fallback="sites" />
        </span>
      )}
      {large && (
        <button
          type="button"
          className="site-thumb-retake"
          disabled={taking}
          title={t("Take a new picture of the home page (the web server must be running)")}
          onClick={retake}
        >
          {taking ? t("Taking a picture…") : t("New picture")}
        </button>
      )}
    </div>
  );
}
