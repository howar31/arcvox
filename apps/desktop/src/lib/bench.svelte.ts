import { benchCancel, benchStart, onBenchDone, onBenchProgress, type BenchRun } from "./api";

/**
 * The speed test run. It lives outside the page, so a run in progress, its results and the
 * chosen engines are still there when the user leaves the page and comes back.
 */
class BenchStore {
  running = $state(false);
  remaining = $state(0);
  runs = $state<BenchRun[]>([]);
  /** Engines ticked for the test; null until the page has chosen its defaults. */
  asr = $state<string[] | null>(null);
  llm = $state<string[] | null>(null);
  #watching = false;

  /** Listens for progress once, for as long as the window lives. */
  watch() {
    if (this.#watching) return;
    this.#watching = true;
    onBenchProgress(({ run, remaining }) => this.progress(run, remaining));
    onBenchDone(() => this.done());
  }

  progress(run: BenchRun, remaining: number) {
    this.runs = [...this.runs, run];
    this.remaining = remaining;
  }

  done() {
    this.running = false;
    this.remaining = 0;
  }

  /** Each engine is run twice: cold, then warm. */
  async start(asr: string[], llm: string[], sentence: string) {
    this.watch();
    this.runs = [];
    this.running = true;
    this.remaining = 2 * (asr.length + llm.length);
    try {
      await benchStart(asr, llm, sentence);
    } catch (e) {
      this.done();
      throw e;
    }
  }

  cancel() {
    return benchCancel();
  }
}

export const bench = new BenchStore();
