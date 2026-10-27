import { errorMessage, modelsCancel, modelsDownload, nativeRefresh, onModelDownloadProgress, setDefaultEngine, upsertProfile, type Config, type DownloadProgress } from "./api";
import type { Role } from "./engines";
import { APPLE_ENGINES, GUIDE_MODELS, type EngineChoice } from "./guideView";
import { defaultProfile } from "./labels";

/**
 * The first-launch guide's state. It lives outside the page: the downloads it started go on
 * while the user is on a later step, and what they chose is still there after a step back.
 */
class GuideStore {
  step = $state<1 | 2 | 3>(1);
  choice = $state<EngineChoice>("models");
  picked = $state<Record<Role, string>>({ asr: GUIDE_MODELS.asr[0], llm: GUIDE_MODELS.llm[0] });
  /** The chosen downloads were begun; false again after a cancel or a failure. */
  started = $state(false);
  /** The model downloading now. */
  current = $state<string | null>(null);
  progress = $state<Record<string, DownloadProgress>>({});
  error = $state<string | null>(null);
  launchAtLogin = $state(true);
  /** When the guide opened, in seconds; a dictation made since counts as the trial. */
  openedAt = Math.floor(Date.now() / 1000);
  #watching = false;
  #cancelling = false;

  /** Listens for download progress once, for as long as the window lives. */
  watch() {
    if (this.#watching) return;
    this.#watching = true;
    onModelDownloadProgress((p) => { this.progress = { ...this.progress, [p.id]: p }; });
  }

  /** Downloads the chosen models one after the other; each becomes the default engine of its role as it arrives. */
  async download(wanted: { role: Role; id: string }[], installed: Set<string>, config: () => Config | null) {
    this.watch();
    this.started = true;
    this.error = null;
    this.#cancelling = false;
    try {
      for (const { role, id } of wanted) {
        if (!installed.has(id)) {
          this.current = id;
          await modelsDownload(id);
        }
        await this.makeDefault(role, id, config());
      }
    } catch (e) {
      if (!this.#cancelling) this.error = errorMessage(e);
      this.started = false;
    } finally {
      this.current = null;
    }
  }

  cancel() {
    this.#cancelling = true;
    if (this.current) modelsCancel(this.current).catch(() => {});
    this.started = false;
  }

  /** Starts over from the first step, for a guide opened again; downloads under way keep their state. */
  reset() {
    if (this.started) return;
    this.step = 1;
    this.choice = "models";
    this.picked = { asr: GUIDE_MODELS.asr[0], llm: GUIDE_MODELS.llm[0] };
    this.progress = {};
    this.error = null;
    this.openedAt = Math.floor(Date.now() / 1000);
  }

  /** Makes the chosen models, all of them installed, the default pair. */
  async useModels(wanted: { role: Role; id: string }[], config: Config | null) {
    let c = config;
    for (const { role, id } of wanted) c = await this.makeDefault(role, id, c);
  }

  /** Looks for the Apple services again and makes them the default pair. */
  async useApple(config: Config | null) {
    const found = await nativeRefresh();
    if (!found.apple_foundation_models) throw new Error("apple_fm_missing");
    const c = await this.makeDefault("asr", APPLE_ENGINES.asr, config);
    await this.makeDefault("llm", APPLE_ENGINES.llm, c);
  }

  /** Makes `id` the default engine of `role`; a default profile naming another engine follows the default again. */
  async makeDefault(role: Role, id: string, config: Config | null): Promise<Config | null> {
    let c = config;
    if (c?.default_engines[role] !== id) c = await setDefaultEngine(role, id);
    const profile = defaultProfile(c);
    const key = role === "asr" ? "asr_provider" : "llm_provider";
    if (profile && profile[key] !== null && profile[key] !== id) c = await upsertProfile({ ...profile, [key]: null });
    return c;
  }
}

export const guide = new GuideStore();
