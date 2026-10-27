import { Component, type ReactNode } from "react";
import { useT } from "../lib/i18n";
import { errorDetails, logToApp, openAppLog } from "../lib/applog";

/**
 * Catches an error while drawing what it holds. Without it React drops the
 * whole page, and the transparent window shows nothing at all. The error
 * goes to the log file; this shows it with a way to go on.
 */
export class Crashed extends Component<{ children: ReactNode; where: string; full?: boolean }, { error: unknown }> {
  state = { error: null as unknown };

  static getDerivedStateFromError(error: unknown) {
    return { error: error ?? "unknown error" };
  }

  componentDidCatch(error: unknown, info: { componentStack?: string | null }) {
    logToApp("error", `${this.props.where} crashed: ${errorDetails(error)}${info.componentStack ? `\ncomponents:${info.componentStack}` : ""}`);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return <CrashView error={this.state.error} full={this.props.full} onRetry={() => this.setState({ error: null })} />;
  }
}

function CrashView({ error, full, onRetry }: { error: unknown; full?: boolean; onRetry: () => void }) {
  const t = useT();
  const text = error instanceof Error ? error.message : String(error);
  const body = (
    <section className="card stack crashed" role="alert">
      <h2>{t("Something went wrong showing this")}</h2>
      <p className="muted small">{t("Stackyard keeps running. The error is in the log file; try again, or reload the window.")}</p>
      <pre className="mono small crashed-text">{text}</pre>
      <div className="row wrap">
        <button type="button" className="btn primary" onClick={onRetry}>
          {t("Try again")}
        </button>
        <button type="button" className="btn" onClick={() => window.location.reload()}>
          {t("Reload the window")}
        </button>
        <button type="button" className="btn" onClick={() => openAppLog().catch(() => {})}>
          {t("Open log file")}
        </button>
      </div>
    </section>
  );
  return full ? <div className="frame crashed-frame">{body}</div> : body;
}
