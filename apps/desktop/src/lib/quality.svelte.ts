import { onQualityDone, onQualityProgress, qualityCancel, qualityCurrent, qualityStart, type EvalStage, type QualityProgress, type QualityRun, type QualitySource } from "./api";
import type { PeriodSel } from "./historyView";

/** Which part of the quality page is shown. */
export type QualityView = "setup" | "result" | "compare";

/**
 * The quality test: the page's choices and the run. It lives outside the page, so a run in
 * progress, its rows and the choices are still there when the user leaves and comes back.
 * The rows come from the backend's record of the run, so the page can be opened mid-run.
 */
class QualityStore {
  view = $state<QualityView>("setup");
  /** The saved report shown on the result view, or null for the run just made. */
  opened = $state<string | null>(null);
  /** The two reports the compare view sets side by side. */
  compare = $state<{ before: string | null; after: string | null }>({ before: null, after: null });
  source = $state<"history" | "file">("history");
  period = $state<PeriodSel>({ kind: "30d" });
  limit = $state(120);
  file = $state<string | null>(null);
  stage = $state<EvalStage>("llm");
  /** The profile the run follows; null until the page chose the default. */
  profile = $state<string | null>(null);
  /** Engines ticked; null until the page has chosen its defaults. */
  asr = $state<string[] | null>(null);
  llm = $state<string[] | null>(null);
  run = $state<QualityRun | null>(null);
  #watching = false;

  get running() { return this.run?.running ?? false; }

  /** Listens for progress once, for as long as the window lives; takes the run as the backend has it. */
  async watch() {
    if (this.#watching) return;
    this.#watching = true;
    onQualityProgress((p) => this.progress(p));
    onQualityDone((d) => this.done(d));
    await this.refresh();
  }

  /** The run as the backend has it (rows and totals included). */
  async refresh() {
    try { this.run = await qualityCurrent(); } catch { /* the engine is not open; the page says so */ }
  }

  progress(p: QualityProgress) {
    if (!this.run) return;
    this.run = { ...this.run, done: p.done, total: p.total, provider: p.provider, items: [...this.run.items, p.item], summaries: p.summaries };
  }

  async done(d: { stopped: boolean; error: string | null }) {
    if (this.run) this.run = { ...this.run, running: false, stopped: d.stopped, error: d.error };
    // The totals are the backend's; take them with the final state.
    await this.refresh();
    if (this.run && !this.run.error) { this.opened = null; this.view = "result"; }
  }

  async start(args: { source: QualitySource; stage: EvalStage; profile: string; asr: string[]; llm: string[] }) {
    await this.watch();
    await qualityStart(args);
    await this.refresh();
  }

  cancel() {
    return qualityCancel();
  }

  show(view: QualityView) { this.view = view; }
}

export const quality = new QualityStore();
