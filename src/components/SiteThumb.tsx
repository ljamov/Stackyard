import { useEffect, useState } from "react";
import { api, errorText, type Site } from "../lib/api";
import { siteBrand } from "../lib/brands";
import { useT } from "../lib/i18n";
import { BrandIcon } from "./BrandIcon";
import { Icon } from "./Icon";
import { IconButton } from "./IconButton";
import "./site-views.css";

/** Pictures already fetched this session, by domain; null = the site has none. */
const pictures = new Map<string, string | null>();
const listeners = new Set<(domain: string) => void>();
/** One fetch per domain at a time, shared by every picture of that site. */
const pending = new Map<string, Promise<string | null>>();

/** Takes a new picture; unlike `load`, a failure is thrown, saying why. */
async function take(domain: string) {
  const url = await api.siteThumbnail(domain, true);
  pictures.set(domain, url ?? pictures.get(domain) ?? null);
  listeners.forEach((l) => l(domain));
}

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
 * the site's logo stands in: its framework's, else its runtime's. A camera
 * button over it takes a new picture and says why when it can't.
 */
export function SiteThumb({ site, large = false }: { site: Site; large?: boolean }) {
  const t = useT();
  const [url, setUrl] = useState(() => pictures.get(site.domain) ?? null);
  const [taking, setTaking] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);

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
    setFailed(null);
    try {
      await take(site.domain);
    } catch (e) {
      setFailed(errorText(e));
    }
    setTaking(false);
  }

  const label = taking
    ? t("Taking a picture…")
    : (failed ?? (url ? t("Take a new picture of the home page") : t("Take a picture of the home page")));
  return (
    <div className={large ? "site-thumb large" : "site-thumb"} data-empty={url ? undefined : ""}>
      {url ? (
        <img src={url} alt={t("The home page of {site}", { site: site.domain })} loading="lazy" draggable={false} />
      ) : (
        <span className="site-thumb-empty" aria-hidden="true">
          <BrandIcon id={siteBrand(site) ?? ""} fallback="sites" />
        </span>
      )}
      {large ? (
        <button type="button" className="site-thumb-retake" disabled={taking} title={label} onClick={retake}>
          <Icon name="camera" />
          {taking ? t("Taking a picture…") : url ? t("New picture") : t("Take picture")}
        </button>
      ) : (
        <span className="site-thumb-take" data-busy={taking || undefined} data-failed={failed ? "" : undefined}>
          <IconButton icon="camera" label={label} busy={taking} onClick={retake} />
        </span>
      )}
      {large && failed && (
        <span className="site-thumb-failed" role="alert">
          {failed}
        </span>
      )}
    </div>
  );
}
