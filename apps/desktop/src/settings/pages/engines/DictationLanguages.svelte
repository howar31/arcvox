<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, nativeDictationLanguages, nativeReleaseLanguage, nativeReserveLanguage, type Config, type DictationLanguages, type ProviderConfig } from "../../../lib/api";
  import { wantsSearch } from "../../../lib/dropdown";
  import { t } from "../../../lib/i18n";
  import { languageName, shownLanguages } from "../../../lib/languages";
  import { nativeStates } from "../../../lib/nativeStates.svelte";
  import { dictationInstall } from "../../../lib/dictationInstall.svelte";
  import AddRow from "../../../lib/ui/AddRow.svelte";
  import Dropdown from "../../../lib/ui/Dropdown.svelte";
  import Fold from "../../../lib/ui/Fold.svelte";
  import Icon from "../../../lib/ui/Icon.svelte";
  import MenuOption from "../../../lib/ui/MenuOption.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";

  /** The languages of Apple dictation under its engine row: each one's state, the profiles that use it, download and removal. */
  let { engine, config }: { engine: ProviderConfig; config: Config | null } = $props();

  const FOLD_KEY = "arcvox.engines.dictation_languages";
  let open = $state((() => { try { return localStorage.getItem(FOLD_KEY) !== "closed"; } catch { return true; } })());
  function toggle() {
    open = !open;
    try { localStorage.setItem(FOLD_KEY, open ? "open" : "closed"); } catch { /* The fold is not remembered. */ }
  }

  /** The language the system is asked for, as the app asks it: no hint, or a bare `zh`, is Taiwan Mandarin. */
  const hintOf = (h: string | null) => (!h || h.trim() === "" || h.trim().toLowerCase() === "zh" ? "zh-TW" : h.trim());

  // The profiles whose speech engine is this one, named or followed as the default.
  const users = $derived((config?.profiles ?? []).filter((p) => p.asr_provider === engine.id || (p.asr_provider === null && config?.default_engines.asr === engine.id)));
  const hints = $derived([...new Set(users.map((p) => hintOf(p.language_hint)))]);

  let langs = $state<DictationLanguages | null>(null);
  let failed = $state<string | null>(null);
  async function load() {
    try {
      langs = await nativeDictationLanguages(hints);
      failed = null;
    } catch (e) {
      failed = errorMessage(e);
    }
  }
  $effect(() => { void hints.join(); load(); });
  onMount(() => {
    const again = () => load();
    window.addEventListener("focus", again);
    return () => { window.removeEventListener("focus", again); };
  });

  /** Each used language as the system resolves it; null for one the system does not support. */
  const usedLocales = $derived(users.map((p) => ({ profile: p, locale: langs?.resolved[hintOf(p.language_hint)] ?? null })));
  const unsupported = $derived(usedLocales.filter((u) => u.locale === null).map((u) => u.profile.name));
  // The languages kept for the app first, the ones counted toward the limit, each part in name order.
  const shown = $derived(langs ? shownLanguages(langs, usedLocales.flatMap((u) => (u.locale ? [u.locale] : []))).sort((a, b) => Number(langs!.reserved.includes(b)) - Number(langs!.reserved.includes(a))) : []);
  const usersOf = (locale: string) => usedLocales.filter((u) => u.locale === locale).map((u) => u.profile.name);
  const full = $derived(!!langs && langs.reserved.length >= langs.max);
  const addable = $derived(langs ? langs.supported.filter((l) => !shown.includes(l)) : []);
  let query = $state("");
  const matches = $derived(addable.filter((l) => !query.trim() || `${languageName(l)} ${l}`.toLowerCase().includes(query.trim().toLowerCase())));

  const STATUSES = ["needs_download", "locale_unsupported", "os_too_old", "failed"];
  const installing = $derived(dictationInstall.locale);
  const fraction = $derived(dictationInstall.fraction);
  const install = (locale: string) => dictationInstall.run(locale);
  // A download that finished, here or while the page was away, changes the list.
  $effect(() => { if (dictationInstall.done > 0) load(); });

  /** Adds a language to the list, kept for this app and not downloaded until asked. */
  async function add(locale: string) {
    try {
      await nativeReserveLanguage(locale);
    } catch (e) {
      const m = errorMessage(e);
      toast.error(STATUSES.includes(m) ? t(`engines.native.${m}`) : m);
    }
    await load();
  }

  // Removal takes a second press within a few seconds; Esc takes the first back.
  let sure = $state<string | null>(null);
  let sureTimer: ReturnType<typeof setTimeout> | undefined;
  function askRemove(locale: string) {
    sure = locale;
    clearTimeout(sureTimer);
    sureTimer = setTimeout(() => (sure = null), 4000);
  }
  async function remove(locale: string) {
    sure = null;
    try {
      await nativeReleaseLanguage(locale);
    } catch (e) {
      toast.error(errorMessage(e));
    }
    await load();
    nativeStates.refresh();
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && sure) { e.stopPropagation(); sure = null; }
  }
</script>

<svelte:window {onkeydown} />

<button type="button" class="foldrow" aria-expanded={open} onclick={toggle}>
  <span class="chev" class:open><Icon name="chevron-right" size={14} /></span>
  <span>{t("engines.languages")}</span>
  {#if langs}<span class="sub">{t("engines.languages_count", { count: String(langs.reserved.length), max: String(langs.max) })}</span>{/if}
</button>
{#if open}
  <Fold>
    {#if failed}<p class="line warn">{failed}</p>{/if}
    {#each shown as l (l)}
      {@const have = !!langs?.installed.includes(l)}
      {@const kept = !!langs?.reserved.includes(l)}
      {@const names = usersOf(l)}
      <div class="lang">
        <span class="ring" class:ok={have} class:warn={!have && names.length > 0}></span>
        <div class="info">
          <div class="title">
            {languageName(l)}<Tag>{l}</Tag>
            {#if kept}<Tag tone="accent">{t("engines.language_kept")}</Tag>{:else if have}<Tag title={t("engines.language_system_tip")}>{t("engines.language_system")}</Tag>{/if}
            {#if !have}<Tag tone="warn">{t("engines.language_missing")}</Tag>{/if}
          </div>
          <small>{names.length > 0 ? t("engines.in_use", { names: names.join(t("engines.list_sep")) }) : t("engines.not_in_use")}</small>
        </div>
        {#if !have}
          <button type="button" class="btn" disabled={installing !== null || (!kept && full)} onclick={() => install(l)}>
            {installing === l ? t("engines.installing", { percent: `${Math.round(fraction * 100)}%` }) : t("engines.download")}
          </button>
        {:else if !kept}
          <button type="button" class="btn" disabled={full} onclick={() => add(l)}>{t("engines.language_keep")}</button>
        {/if}
        {#if kept && installing !== l}
          {#if sure === l}
            <button type="button" class="btn danger" onclick={() => remove(l)}>{t("engines.language_remove_sure")}</button>
          {:else}
            <button type="button" class="btn quiet danger" onclick={() => askRemove(l)}>{t("engines.language_remove")}</button>
          {/if}
        {/if}
      </div>
    {/each}
    {#if unsupported.length > 0}
      <p class="line warn">{t("engines.language_unsupported", { names: unsupported.join(t("engines.list_sep")) })}</p>
    {/if}
    {#if langs}
      <AddRow>
        <Dropdown label={t("engines.language_add")} search={wantsSearch(addable.length, false)} bind:query placeholder={t("engines.language_search")} disabled={full || installing !== null} minWidth={280}>
          {#snippet button()}<span class="ph">{full ? t("engines.languages_full", { max: String(langs!.max) }) : t("engines.language_add")}</span>{/snippet}
          {#snippet list(close)}
            {#each matches as l (l)}
              <MenuOption onclick={() => { close(); add(l); }}><span class="opt"><span>{languageName(l)}</span><small>{l}</small></span></MenuOption>
            {/each}
          {/snippet}
        </Dropdown>
      </AddRow>
    {/if}
  </Fold>
{/if}

<style>
  .foldrow { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 48px; padding: 0 var(--row-x, 20px); border: 0; border-top: 1px solid var(--line); background: none; color: var(--ink); font-size: var(--text-control); font-weight: 600; text-align: left; cursor: default; }
  .foldrow .sub { color: var(--ink-2); font-weight: 400; font-size: var(--text-sm); }
  .foldrow .chev { display: inline-flex; color: var(--ink-3); transition: transform .2s; }
  .foldrow .chev.open { transform: rotate(90deg); }
  .lang { display: flex; align-items: center; gap: 14px; padding: 10px var(--row-x, 20px) 10px 49px; border-top: 1px solid var(--line); }
  .ring { flex: none; width: 13px; height: 13px; border-radius: 50%; border: 2px solid var(--ink-3); }
  .ring.ok { border-color: var(--ok); }
  .ring.warn { border-color: var(--warn); }
  .info { flex: 1; min-width: 0; }
  .title { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; font-size: var(--text-control); }
  .info small { display: block; color: var(--ink-2); font-size: var(--text-sm); }
  .line { margin: 0; padding: 8px var(--row-x, 20px) 8px 49px; border-top: 1px solid var(--line); font-size: var(--text-sm); }
  .line.warn { color: var(--warn); }
  .ph { color: var(--ink-2); }
  .opt { display: flex; justify-content: space-between; gap: 12px; width: 100%; }
  .opt small { color: var(--ink-3); }
</style>
