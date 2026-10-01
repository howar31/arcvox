<script lang="ts">
  import { onMount } from "svelte";
  import { clearHistory, deleteHistory, errorMessage, listHistory, widgetCopy, type Utterance } from "../../lib/api";
  import { groupByDay, seconds, showsRaw, totalSeconds } from "../../lib/historyView";
  import { formatDateTime, formatDay, formatTime } from "../../lib/format";
  import { t } from "../../lib/i18n";
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
  function dayTitle(key: string): string {
    if (key === "today" || key === "yesterday") return t(`history.${key}`);
    const [y, m, d] = key.split("-").map(Number);
    return formatDay(new Date(y, m - 1, d), now);
  }

  /** The profile's name; for a profile that no longer exists, its id and a deleted marker. */
  function profileOf(id: string): { name: string; deleted: boolean } {
    const p = config.value?.profiles.find((x) => x.id === id);
    return p ? { name: p.name, deleted: false } : { name: id, deleted: !!config.value && !!id };
  }
  type Part = { kind: "text"; text: string } | { kind: "profile"; name: string; deleted: boolean } | { kind: "degraded" };
  /** The meta line: application, profile, the not-rewritten tag, and the time taken. */
  function parts(u: Utterance): Part[] {
    const out: Part[] = [];
    if (u.app_name) out.push({ kind: "text", text: u.app_name });
    if (u.profile_id) out.push({ kind: "profile", ...profileOf(u.profile_id) });
    if (u.degraded) out.push({ kind: "degraded" });
    const asr = seconds(u.asr_ms);
    const llm = seconds(u.llm_ms);
    const total = totalSeconds(u.asr_ms, u.llm_ms);
    if (asr !== null && llm !== null && total !== null) out.push({ kind: "text", text: t("history.total_secs", { total, asr, llm }) });
    else if (asr !== null) out.push({ kind: "text", text: t("history.asr_secs", { secs: asr }) });
    return out;
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
        <Row>
          <span class="time" title={formatDateTime(u.ts)}>{formatTime(u.ts)}</span>
          <span class="body">
            <span class="final">{u.final_text}</span>
            {#if showsRaw(u)}<span class="raw">{t("history.raw_line", { text: u.raw_text.trim() })}</span>{/if}
            <span class="meta">
              {#each parts(u) as part, j (j)}
                <!-- The separator stays with the part after it when the line wraps. -->
                <span class="part">
                  {#if j > 0}<span class="dot" aria-hidden="true">·</span>{/if}
                  {#if part.kind === "degraded"}<Tag tone="warn">{t("history.not_rewritten")}</Tag>
                  {:else if part.kind === "profile"}<span class="meta-text">{part.name}{#if part.deleted}<span class="deleted">{t("history.deleted_profile")}</span>{/if}</span>
                  {:else}<span class="meta-text">{part.text}</span>{/if}
                </span>
              {/each}
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
  .meta { display: flex; align-items: center; gap: 2px 6px; flex-wrap: wrap; color: var(--ink-2); font-size: 12px; font-variant-numeric: tabular-nums; }
  .meta-text { min-width: 0; overflow-wrap: anywhere; }
  .part { display: inline-flex; align-items: center; gap: 6px; min-width: 0; }
  .dot { color: var(--ink-3); }
  .deleted { margin-left: 3px; color: var(--ink-3); }
  .tools { flex: none; align-self: flex-start; display: flex; gap: 2px; margin-right: -6px; }
  .icon-btn { width: 26px; padding: 0; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
</style>
