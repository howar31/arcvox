import { errorMessage, getClient, getConfig, getDefaults, onConfigChanged, onWidgetIdle, updateClient, type ClientSettings, type Config, type Defaults } from "./api";

class ConfigStore {
  value = $state<Config | null>(null);
  error = $state<string | null>(null);
  async load() {
    try { this.value = await getConfig(); this.error = null; } catch (e) { this.error = errorMessage(e); }
  }
  async watch() {
    return onConfigChanged((c) => { this.value = c; });
  }
}

class ClientStore {
  value = $state<ClientSettings | null>(null);
  async load() { this.value = await getClient(); }
  async save(next: ClientSettings) { this.value = await updateClient(next); }
  /** Follows what a menu changes outside the settings window. */
  async watch() {
    return onWidgetIdle((idle) => { if (this.value && this.value.widget_idle !== idle) this.value = { ...this.value, widget_idle: idle }; });
  }
}

/** The settings of a new installation; null until loaded, and then no setting offers a reset. */
class DefaultsStore {
  value = $state<Defaults | null>(null);
  async load() { try { this.value = await getDefaults(); } catch { this.value = null; } }
}

export const config = new ConfigStore();
export const client = new ClientStore();
export const defaults = new DefaultsStore();
