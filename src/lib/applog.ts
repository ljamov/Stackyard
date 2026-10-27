import { invoke, isTauri } from "@tauri-apps/api/core";

/**
 * The app's log file (`stackyard.log`, by the installed program on
 * Windows): errors from this page go there too, so a window that breaks
 * leaves a trace.
 */
export function logToApp(level: "error" | "warn" | "info", message: string) {
  if (!isTauri()) return;
  invoke("app_log", { level, message }).catch(() => {});
}

function describe(value: unknown) {
  if (value instanceof Error) return value.stack || `${value.name}: ${value.message}`;
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

/** Sends the page's uncaught errors to the log. Call once, first thing. */
export function catchPageErrors() {
  window.addEventListener("error", (e) =>
    logToApp("error", `${e.message} (${e.filename}:${e.lineno}:${e.colno})${e.error instanceof Error && e.error.stack ? `\n${e.error.stack}` : ""}`),
  );
  window.addEventListener("unhandledrejection", (e) => logToApp("warn", `unhandled: ${describe(e.reason)}`));
}

export function errorDetails(error: unknown) {
  return describe(error);
}

/** Opens the log in the system's text viewer; returns where it is. */
export function openAppLog() {
  return invoke<string>("app_log_open");
}

export function appLogPath(): Promise<string | null> {
  return isTauri() ? invoke<string | null>("app_log_path") : Promise.resolve(null);
}
