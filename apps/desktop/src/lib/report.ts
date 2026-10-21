import { invoke } from "@tauri-apps/api/core";

/** Sends the page's uncaught errors and rejected promises to the app's log, so a page that breaks leaves a line behind. */
export function reportErrors(window: "settings" | "widget" | "panel"): void {
  const send = (message: string, source?: string, stack?: string) => {
    invoke("webview_error", { window, message, source, stack }).catch(() => { /* The log is out of reach; nothing else to do. */ });
  };
  addEventListener("error", (e) => send(e.message, e.filename ? `${e.filename}:${e.lineno}:${e.colno}` : undefined, e.error?.stack));
  addEventListener("unhandledrejection", (e) => {
    const r = e.reason;
    send(r instanceof Error ? r.message : String(r), undefined, r instanceof Error ? r.stack : undefined);
  });
}
