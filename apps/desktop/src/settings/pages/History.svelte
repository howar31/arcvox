<script lang="ts">
  import { onMount } from "svelte";
  import { clearHistory, deleteHistory, errorMessage, listHistory, widgetCopy, type Utterance } from "../../lib/api";
  import { groupByDay, seconds, showsRaw } from "../../lib/historyView";
  import { currentLanguage, t } from "../../lib/i18n";
  import { profileName } from "../../lib/labels";
  import { config } from "../../lib/store.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  let rows = $state<Utterance[] | null>(null);
  let now = $state(new Date());
  let confirmClear = $state(false);
  // Set when the list could not be read and nothing has been shown yet.
  let failed = $state(false);

  async function reload() {
    try {
      rows = await listHistory(200);
      now = new Date();
      failed = false;
    } catch (e) {
      if (rows === null) failed = true;
      toast.error(errorMessage(e));
    }
  }

  onMount(() => {
    reload();
    const onFocus = () => { reload(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  const groups = $derived(groupByDay(rows ?? [], now));
  const locale = $derived(currentLanguage() === "zh-TW" ? "zh-TW" : "en");

  function dayTitle(key: string): string {
    if (key === "today" || key === "yesterday") return t(`history.${key}`);
    const [y, m, d] = key.split("-").map(Number);
    const date = new Date(y, m - 1, d);
    const opts: Intl.DateTimeFormatOptions = { month: "long", day: "numeric", weekday: "short" };
    if (y !== now.getFullYear()) opts.year = "numeric";
    return date.toLocaleDateString(locale, opts);
  }

  function time(ts: number): { short: string; full: string } {
    const d = new Date(ts * 1000);
    return {
      short: d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
      full: d.toLocaleString(locale),
    };
  }

  const sep = () => t("home.meta_sep");
  function source(u: Utterance): string {
    return [u.app_name, profileName(config.value, u.profile_id)].filter(Boolean).join(sep());
  }
  function timings(u: Utterance): string {
    const asr = seconds(u.asr_ms);
    const llm = seconds(u.llm_ms);
    return [
      asr === null ? null : t("history.asr_secs", { secs: asr }),
      llm === null ? null : t("history.llm_secs", { secs: llm }),
    ].filter(Boolean).join(sep());
  }

  async function copy(text: string) {
    try { await widgetCopy(text); toast.ok(t("home.copied")); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function remove(id: number) {
    try { await deleteHistory(id); } catch (e) { toast.error(errorMessage(e)); }
    await reload();
  }

  async function clearAll() {
    confirmClear = false;
    try {
      await clearHistory();
      toast.ok(t("history.cleared"));
    } catch (e) {
      toast.error(errorMessage(e));
    }
    await reload();
  }
</script>

<PageHead
  title={t("nav.history")}
  description={config.value ? t("history.retention", { days: String(config.value.general.history_retention_days) }) : undefined}
>
  {#snippet actions()}
    <button type="button" class="btn" onclick={reload}><Icon name="refresh" size={14} />{t("history.reload")}</button>
    <button type="button" class="btn danger" disabled={!rows?.length} onclick={() => (confirmClear = true)}>{t("history.clear_all")}</button>
  {/snippet}
</PageHead>

{#if failed}
  <Group>
    <Empty text={t("history.load_failed")}>
      {#snippet action()}<button type="button" class="btn" onclick={reload}>{t("history.retry")}</button>{/snippet}
    </Empty>
  </Group>
{:else if rows !== null}
  {#each groups as g (g.key)}
    <Group title={dayTitle(g.key)}>
      {#each g.rows as u (u.id)}
        {@const at = time(u.ts)}
        <Row>
          <span class="time" title={at.full}>{at.short}</span>
          <span class="body">
            <span class="final">{u.final_text}</span>
            {#if showsRaw(u)}<span class="raw">{t("history.raw_line", { text: u.raw_text.trim() })}</span>{/if}
            <span class="meta">
              {#if u.degraded}
                <span class="meta-text">{source(u)}</span>
                <Tag tone="warn">{t("history.not_rewritten")}</Tag>
                <span class="meta-text">{timings(u)}</span>
              {:else}
                <span class="meta-text">{[source(u), timings(u)].filter(Boolean).join(sep())}</span>
              {/if}
            </span>
          </span>
          <span class="tools">
            <button type="button" class="btn quiet icon-btn" aria-label={t("widget.copy")} title={t("widget.copy")} onclick={() => copy(u.final_text)}><Icon name="copy" /></button>
            <button type="button" class="btn quiet icon-btn" aria-label={t("history.delete")} title={t("history.delete")} onclick={() => remove(u.id)}><Icon name="trash" /></button>
          </span>
        </Row>
      {/each}
    </Group>
  {:else}
    <Group>
      <Empty text={t("history.empty")} />
    </Group>
  {/each}
{/if}

<Modal bind:open={confirmClear} title={t("history.confirm_clear")}>
  <p>{t("history.clear_body")}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (confirmClear = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={clearAll}>{t("history.clear_all")}</button>
  {/snippet}
</Modal>

<style>
  .time { width: 44px; flex: none; align-self: flex-start; padding-top: 1px; color: var(--ink-2); font-size: 12px; font-variant-numeric: tabular-nums; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; padding: 2px 0; }
  .final { line-height: 1.55; overflow-wrap: anywhere; white-space: pre-line; }
  .raw { color: var(--ink-2); font-size: 12px; overflow-wrap: anywhere; }
  .meta { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; color: var(--ink-2); font-size: 12px; font-variant-numeric: tabular-nums; }
  .meta-text { min-width: 0; overflow-wrap: anywhere; }
  .tools { flex: none; align-self: flex-start; display: flex; gap: 2px; margin-right: -6px; }
  .icon-btn { width: 26px; padding: 0; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
</style>
