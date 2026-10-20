<script lang="ts">
  /** What the records take on this computer, one line per kind with its deletion: the history entries, and
   * under them the recordings, stage records and reruns that hang on them; the statistics on their own. */
  import { clearAttempts, clearHistory, clearRecordings, clearStages, errorMessage, formatBytes, historyStorage, resetStats, type HistoryStorage } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { toast } from "../../lib/ui/toast.svelte";
  import Group from "../../lib/ui/Group.svelte";

  type Kind = "rows" | "recordings" | "stages" | "attempts" | "stats";

  let storage = $state<HistoryStorage | null>(null);
  const load = () => historyStorage().then((s) => { storage = s; }).catch(() => { storage = null; });
  $effect(() => { load(); });

  // A first press asks on the line itself; the second within a few seconds deletes.
  let sure = $state<Kind | null>(null);
  let sureTimer: ReturnType<typeof setTimeout> | undefined;
  function ask(kind: Kind) {
    clearTimeout(sureTimer);
    sure = kind;
    sureTimer = setTimeout(() => { if (sure === kind) sure = null; }, 5000);
  }
  function cancel() {
    clearTimeout(sureTimer);
    sure = null;
  }

  async function remove(kind: Kind) {
    cancel();
    try {
      switch (kind) {
        case "rows": await clearHistory(); toast.ok(t("storage.deleted_rows")); break;
        case "recordings": toast.ok(t("storage.deleted_recordings", { n: (await clearRecordings()).toLocaleString() })); break;
        case "stages": toast.ok(t("storage.deleted_stages", { n: (await clearStages()).toLocaleString() })); break;
        case "attempts": toast.ok(t("storage.deleted_attempts", { n: (await clearAttempts()).toLocaleString() })); break;
        case "stats": await resetStats(); toast.ok(t("general.stats_reset_done")); break;
      }
    } catch (e) {
      toast.error(errorMessage(e));
    }
    load();
  }

  const num = (n: number) => n.toLocaleString();
  const total = $derived(storage ? storage.recording_bytes + storage.db_bytes : 0);
  const recShare = $derived(total ? (storage!.recording_bytes / total) * 100 : 0);

  const lines = $derived.by(() => {
    const s = storage;
    if (!s) return [];
    return [
      { kind: "rows" as const, child: false, label: t("storage.rows"), help: t("storage.rows_help"), amount: t("storage.rows_amount", { n: num(s.rows), size: formatBytes(s.text_bytes) }), empty: s.rows === 0, sure: t("storage.sure_rows", { n: num(s.rows), rec: num(s.recordings) }) },
      { kind: "recordings" as const, child: true, label: t("storage.recordings"), help: "", amount: t("storage.recordings_amount", { n: num(s.recordings), size: formatBytes(s.recording_bytes) }), empty: s.recordings === 0, sure: t("storage.sure_recordings", { n: num(s.recordings) }) },
      { kind: "stages" as const, child: true, label: t("storage.stages"), help: "", amount: t("storage.stages_amount", { n: num(s.stages), size: formatBytes(s.stage_bytes) }), empty: s.stages === 0, sure: t("storage.sure_stages", { n: num(s.stages) }) },
      { kind: "attempts" as const, child: true, label: t("storage.attempts"), help: t("storage.attempts_help"), amount: t("storage.attempts_amount", { n: num(s.attempts) }), empty: s.attempts === 0, sure: t("storage.sure_attempts", { n: num(s.attempts) }) },
      { kind: "stats" as const, child: false, label: t("storage.stats"), help: "", amount: t("storage.stats_amount", { n: num(s.stats_hours) }), empty: s.stats_hours === 0, sure: t("storage.sure_stats") },
    ];
  });
</script>

<Group title={t("storage.usage")} help={t("storage.usage_note")}>
  {#if storage}
    <div class="head">
      <div class="total">{t("storage.total", { size: formatBytes(total) })}</div>
      <div class="bar" aria-hidden="true"><i style:width="{recShare}%"></i><i class="rest" style:width="{100 - recShare}%"></i></div>
      <div class="legend"><span><i></i>{t("storage.usage_recordings", { size: formatBytes(storage.recording_bytes) })}</span><span><i class="rest"></i>{t("storage.usage_text", { size: formatBytes(storage.db_bytes) })}</span></div>
    </div>
    {#each lines as l (l.kind)}
      <div class="line" class:child={l.child} class:asking={sure === l.kind}>
        <span class="label">
          <span class="text">{l.label}</span>
          {#if sure === l.kind}<small class="warn">{l.sure}</small>{:else if l.help}<small>{l.help}</small>{/if}
        </span>
        {#if sure === l.kind}
          <button type="button" class="btn quiet w" onclick={cancel}>{t("common.cancel")}</button>
          <button type="button" class="btn danger w" onclick={() => remove(l.kind)}>{t("storage.delete_sure")}</button>
        {:else}
          <span class="amount num">{l.amount}</span>
          <button type="button" class="btn quiet danger w" disabled={l.empty} onclick={() => ask(l.kind)}>{t("storage.delete_all")}</button>
        {/if}
      </div>
    {/each}
  {/if}
</Group>

<style>
  .head { padding: 14px var(--row-x) 12px; border-bottom: 1px solid var(--line); }
  .total { font-size: var(--text-sm); color: var(--ink-2); margin-bottom: 8px; }
  .total, .amount { font-variant-numeric: tabular-nums; }
  .bar { display: flex; height: 8px; border-radius: 4px; overflow: hidden; background: var(--sunken); }
  .bar i { display: block; height: 100%; background: var(--accent); }
  .bar i.rest, .legend i.rest { background: color-mix(in srgb, var(--accent) 40%, transparent); }
  .legend { display: flex; gap: 16px; margin-top: 8px; font-size: var(--text-xs); color: var(--ink-2); }
  .legend i { display: inline-block; width: 10px; height: 10px; border-radius: 3px; margin-right: 6px; vertical-align: -1px; background: var(--accent); }
  .line { display: flex; align-items: center; gap: 8px; min-height: 56px; padding: 10px var(--row-x); border-top: 1px solid var(--line); }
  .head + .line { border-top: 0; }
  .line.child { padding-left: calc(var(--row-x) + 24px); }
  .label { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .text { font-weight: 500; }
  .line:not(.child) .text { font-weight: 600; }
  small { color: var(--ink-2); font-size: var(--text-sm); }
  small.warn { color: var(--warn); }
  .amount { color: var(--ink-2); font-size: var(--text-sm); white-space: nowrap; }
  .btn.w { width: 96px; justify-content: center; }
  .line .btn:last-child { margin-right: -8px; }
</style>
