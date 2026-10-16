import { useEffect, useState } from "react";
import { api, errorText, type InstallProgress, type SetupChoices, type SetupOffer, type SetupStatus } from "../lib/api";
import { Logo } from "../components/Logo";

type Step = "folders" | "modules" | "installing";
type ItemState = { state: "waiting" | "installing" | "done" | "failed"; detail?: string };

/**
 * First-run setup: where Stackyard keeps its files and sites, and what to
 * install. Everything can be changed later in Settings and Services.
 */
export function Setup({ status, onDone }: { status: SetupStatus; onDone(): void }) {
  const [step, setStep] = useState<Step>("folders");
  const [home, setHome] = useState(status.default_home);
  const [sites, setSites] = useState(status.default_sites_folder);
  const [sitesTouched, setSitesTouched] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(status.offers.filter((o) => o.recommended).map((o) => o.id)),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(status.move_error);
  const [items, setItems] = useState<Record<string, ItemState>>({});
  const [progress, setProgress] = useState<Record<string, InstallProgress>>({});

  // After the restart that moved Stackyard's files, carry on installing.
  useEffect(() => {
    if (status.choices) void installAll(status.choices);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const offs: Promise<() => void>[] = [
      api.onInstallProgress((p) => setProgress((all) => ({ ...all, [p.kind]: p }))),
      api.onServiceProgress(({ id, ...p }) =>
        setProgress((all) => ({ ...all, [id]: { ...p, kind: "php", version: "" } })),
      ),
    ];
    return () => offs.forEach((off) => off.then((f) => f()));
  }, []);

  function changeHome(value: string) {
    setHome(value);
    // The sites folder follows Stackyard's folder until it is changed by hand.
    if (!sitesTouched && status.default_sites_folder.startsWith(status.default_home)) {
      setSites(value.replace(/[\\/]+$/, "") + status.default_sites_folder.slice(status.default_home.length));
    }
  }

  async function browse(which: "home" | "sites") {
    const folder = await api.pickFolder(
      which === "home" ? "Select Stackyard's folder" : "Select the folder for your sites",
      which === "home" ? home : sites,
    );
    if (!folder) return;
    if (which === "home") changeHome(folder);
    else {
      setSites(folder);
      setSitesTouched(true);
    }
  }

  async function start() {
    setBusy(true);
    setError(null);
    try {
      const install = status.offers.filter((o) => picked.has(o.id)).map((o) => o.id);
      // When the folder changes, the app restarts and this screen carries on.
      const restarting = await api.setupChoose(home, sites, install);
      if (!restarting) await installAll({ sites_folder: sites, install });
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  }

  async function installAll(choices: SetupChoices) {
    setStep("installing");
    setBusy(true);
    const offers = status.offers.filter((o) => choices.install.includes(o.id));
    setItems(Object.fromEntries(offers.map((o) => [o.id, { state: "waiting" }])));
    try {
      await api.setupApply();
    } catch (e) {
      setError(errorText(e));
    }
    const one = async (o: SetupOffer) => {
      setItems((all) => ({ ...all, [o.id]: { state: "installing" } }));
      try {
        if (o.group === "runtime") await api.installRuntime(o.id as "php" | "node", o.version ?? "");
        else if (o.id === "phpmyadmin") await api.installPhpMyAdmin();
        else await api.installService(o.id);
        setItems((all) => ({ ...all, [o.id]: { state: "done" } }));
      } catch (e) {
        setItems((all) => ({ ...all, [o.id]: { state: "failed", detail: errorText(e) } }));
      }
    };
    // A few at a time, so downloads don't crowd each other out.
    const queue = [...offers];
    await Promise.all(
      [0, 1, 2].map(async () => {
        for (let o = queue.shift(); o; o = queue.shift()) await one(o);
      }),
    );
    setBusy(false);
  }

  async function finish() {
    try {
      await api.setupFinish();
      onDone();
    } catch (e) {
      setError(errorText(e));
    }
  }

  const toggle = (id: string) =>
    setPicked((all) => {
      const next = new Set(all);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const group = (g: SetupOffer["group"]) => status.offers.filter((o) => o.group === g);
  const failed = Object.values(items).some((i) => i.state === "failed");

  return (
    <div className="setup">
      <div className="setup-card card stack">
        <div className="row">
          <Logo size={36} />
          <div>
            <h1>Welcome to Stackyard</h1>
            <p className="muted">
              {step === "folders" && "Step 1 of 2: choose where Stackyard keeps its files and your sites."}
              {step === "modules" && "Step 2 of 2: choose what to install. You can add or remove any of it later."}
              {step === "installing" && "Installing what you picked."}
            </p>
          </div>
        </div>

        {error && (
          <p className="notice error small" role="alert">
            {error}
          </p>
        )}

        {step === "folders" && (
          <>
            <FolderField
              id="setup-home"
              label="Stackyard folder"
              hint="Runtimes, databases, services and logs live here."
              value={home}
              onChange={changeHome}
              onBrowse={() => browse("home")}
            />
            <FolderField
              id="setup-sites"
              label="Sites folder"
              hint="Every folder you create in here becomes a site, like myapp.stack."
              value={sites}
              onChange={(v) => {
                setSites(v);
                setSitesTouched(true);
              }}
              onBrowse={() => browse("sites")}
            />
            {status.existing && home.replace(/[\\/]+$/, "") !== status.existing.replace(/[\\/]+$/, "") && (
              <p className="notice small">
                Stackyard is in <span className="mono">{status.existing}</span> now. Its files move to the new folder,
                and Stackyard restarts once to do that.
              </p>
            )}
            <div className="row setup-actions">
              <button
                type="button"
                className="btn primary"
                disabled={!home.trim() || !sites.trim()}
                onClick={() => setStep("modules")}
              >
                Next
              </button>
            </div>
          </>
        )}

        {step === "modules" && (
          <>
            {(["service", "runtime"] as const).map((g) => (
              <section key={g} className="stack tight" aria-labelledby={`setup-${g}`}>
                <h2 id={`setup-${g}`}>{g === "service" ? "Services" : "Runtimes"}</h2>
                <div className="setup-offers">
                  {group(g).map((o) => (
                    <label key={o.id} className="setup-offer">
                      <input type="checkbox" checked={picked.has(o.id)} onChange={() => toggle(o.id)} />
                      <span className="stack tight">
                        <span className="strong">
                          {o.name}
                          {o.version && <span className="muted mono small"> {o.version}</span>}
                        </span>
                        <span className="muted small">{o.description}</span>
                      </span>
                    </label>
                  ))}
                </div>
              </section>
            ))}
            <div className="row setup-actions">
              <button type="button" className="btn" disabled={busy} onClick={() => setStep("folders")}>
                Back
              </button>
              <button type="button" className="btn primary" disabled={busy} onClick={start}>
                {busy ? "Starting…" : picked.size ? `Install ${picked.size}` : "Continue without installing"}
              </button>
            </div>
          </>
        )}

        {step === "installing" && (
          <>
            <ul className="setup-progress">
              {status.offers
                .filter((o) => items[o.id])
                .map((o) => (
                  <li key={o.id} className="row between">
                    <span className="strong">{o.name}</span>
                    <span className="small">{label(items[o.id], progress[o.id])}</span>
                  </li>
                ))}
              {Object.keys(items).length === 0 && <li className="muted">Nothing to install.</li>}
            </ul>
            {failed && (
              <p className="muted small">
                You can try the ones that failed again from the Services and Runtimes screens.
              </p>
            )}
            <div className="row setup-actions">
              <button type="button" className="btn primary" disabled={busy} onClick={finish}>
                {busy ? "Installing…" : "Open Stackyard"}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function label(item: ItemState, progress?: InstallProgress) {
  switch (item.state) {
    case "waiting":
      return <span className="muted">Waiting</span>;
    case "done":
      return <span className="ok">Installed</span>;
    case "failed":
      return <span className="error">Failed: {item.detail}</span>;
    case "installing": {
      if (progress?.step === "unpacking") return <span className="muted">Unpacking…</span>;
      if (progress?.step === "downloading" && progress.total)
        return <span className="muted">Downloading {Math.floor(((progress.done ?? 0) * 100) / progress.total)}%</span>;
      return <span className="muted">Downloading…</span>;
    }
  }
}

function FolderField(props: {
  id: string;
  label: string;
  hint: string;
  value: string;
  onChange(value: string): void;
  onBrowse(): void;
}) {
  return (
    <div className="field">
      <label htmlFor={props.id}>{props.label}</label>
      <div className="row folder-input">
        <input
          id={props.id}
          className="mono grow"
          value={props.value}
          onChange={(e) => props.onChange(e.target.value)}
        />
        <button type="button" className="btn" onClick={props.onBrowse}>
          Browse…
        </button>
      </div>
      <span className="muted small">{props.hint}</span>
    </div>
  );
}
