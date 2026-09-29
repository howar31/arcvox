import { getClient, getConfig, onConfigChanged, updateClient, type ClientSettings, type Config } from "./api";

export type Page = "general" | "providers" | "models" | "profiles" | "prompt" | "dictionary" | "history" | "about";
export const PAGES: Page[] = ["general", "providers", "models", "profiles", "prompt", "dictionary", "history", "about"];

export function pageFromHash(hash: string): Page {
  const name = hash.replace(/^#/, "");
  return (PAGES as string[]).includes(name) ? (name as Page) : "general";
}

class ConfigStore {
  value = $state<Config | null>(null);
  error = $state<string | null>(null);
  async load() {
    try { this.value = await getConfig(); this.error = null; } catch (e) { this.error = String((e as { message?: string }).message ?? e); }
  }
  async watch() {
    return onConfigChanged((c) => { this.value = c; });
  }
}

class ClientStore {
  value = $state<ClientSettings | null>(null);
  async load() { this.value = await getClient(); }
  async save(next: ClientSettings) { this.value = await updateClient(next); }
}

export const config = new ConfigStore();
export const client = new ClientStore();
