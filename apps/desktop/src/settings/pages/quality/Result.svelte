<script lang="ts">
  /** A finished run or a saved report: totals per engine, then one engine's breakdown. */
  import { errorMessage, qualityOpen, qualitySave, type EvalItem, type EvalSummary, type ReportHead } from "../../../lib/api";
  import { formatDateTime } from "../../../lib/format";
  import { t } from "../../../lib/i18n";
  import { profileName, providerLabel } from "../../../lib/labels";
  import { quality } from "../../../lib/quality.svelte";
  import { clock, providersOf } from "../../../lib/qualityView";
  import { config } from "../../../lib/store.svelte";
  import Group from "../../../lib/ui/Group.svelte";
  import PageHead from "../../../lib/ui/PageHead.svelte";
  import Segmented from "../../../lib/ui/Segmented.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";
  import Breakdown from "./Breakdown.svelte";
  import Tiles from "./Tiles.svelte";

  let { onsaved }: { onsaved: (head: ReportHead) => void } = $props();

  // A saved report is read when the view opens on it; the run just made is the store's.
  let opened = $state<{ name: string; created: number; notes: string[]; stopped: boolean; items: EvalItem[]; summaries: EvalSummary[] } | null>(null);
  let name = $state("");
  let saving = $state(false);
  $effect(() => {
    const file = quality.opened;
    opened = null;
    if (!file) return;
    qualityOpen(file).then((r) => {
      opened = { name: r.report.name, created: Number(r.report.created), notes: r.report.notes, stopped: r.report.stopped, items: r.report.items, summaries: r.summaries };
    }).catch((e) => toast.error(errorMessage(e)));
  });

  const run = $derived(quality.run);
  const items = $derived(opened ? opened.items : run?.items ?? []);
  const summaries = $derived(opened ? opened.summaries : run?.summaries ?? []);
  const stage = $derived(summaries[0]?.stage ?? run?.stage ?? "llm");
  const stopped = $derived(opened ? opened.stopped : run?.stopped ?? false);
  const saved = $derived(opened !== null || !!run?.saved);
  let picked = $state<string | null>(null);
  const providers = $derived(summaries.map((s) => s.provider));
  const current = $derived(summaries.find((s) => s.provider === picked) ?? summaries[0] ?? null);
  const label = (p: string) => providersOf(p, stage).map((id) => providerLabel(config.value, id)).join(" + ");
  const meta = $derived.by(() => {
    const parts = [t(`quality.stage.${stage}`)];
    if (run && !opened) parts.push(profileName(config.value, run.profile));
    else if (opened) { const p = opened.notes.find((n) => n.startsWith("profile: "))?.slice(9); if (p) parts.push(profileName(config.value, p)); }
    parts.push(t("quality.rows_n", { n: String(run && !opened ? run.rows : Math.round(items.length / Math.max(1, providers.length))) }));
    const when = opened ? opened.created : run?.started ?? 0;
    if (when) parts.push(formatDateTime(when));
    if (run && !opened) parts.push(t("quality.took", { clock: clock(run.elapsed_ms) }));
    return parts.join("，");
  });

  async function save() {
    saving = true;
    try {
      const head = await qualitySave(name);
      await quality.refresh();
      onsaved(head);
      toast.ok(t("quality.saved_as", { name: head.name }));
    } catch (e) { toast.error(errorMessage(e)); } finally { saving = false; }
  }
</script>

<PageHead title={opened ? opened.name : t("quality.result")} description={meta} crumb={[{ label: t("nav.engines"), page: "engines" }, { label: t("quality.title"), page: "quality" }]} />

{#if summaries.length === 0}
  <Group><p class="help">{t("quality.no_rows_yet")}</p></Group>
{:else}
  {#if !opened}
    <Group>
      <div class="save">
        {#if stopped}<Tag tone="warn">{t("quality.stopped")}</Tag>{/if}
        {#if run?.saved}
          <span class="sub">{t("quality.saved_as", { name: run.saved.replace(/\.json$/, "") })}</span>
        {:else}
          <input type="text" class="name" placeholder={t("quality.name_placeholder")} bind:value={name} aria-label={t("quality.name_placeholder")} />
          <button type="button" class="btn" onclick={save} disabled={saving || saved}>{t("quality.save")}</button>
        {/if}
        <span class="sp"></span>
        <button type="button" class="btn quiet" onclick={() => quality.show("setup")}>{t("quality.again")}</button>
      </div>
    </Group>
  {:else if stopped}
    <Group><div class="save"><Tag tone="warn">{t("quality.stopped")}</Tag></div></Group>
  {/if}

  <Group title={t("quality.totals")}>
    {#each summaries as s (s.provider)}
      {#if summaries.length > 1}<div class="subhead">{label(s.provider)}</div>{/if}
      <Tiles summary={s} />
    {/each}
  </Group>

  {#if summaries.length > 1}
    <div class="pick">
      <Segmented options={providers.map((p) => ({ value: p, label: label(p) }))} bind:value={() => current?.provider ?? providers[0], (v) => (picked = v)} label={t("quality.engines")} />
    </div>
  {/if}
  {#if current}
    {#key current.provider}
      <Breakdown summary={current} {items} suffix={summaries.length > 1 ? label(current.provider) : ""} />
    {/key}
  {/if}
{/if}

<style>
  .save { display: flex; align-items: center; gap: 10px; padding: 12px var(--row-x); }
  .save .sp { flex: 1; }
  .name { width: 260px; }
  .subhead { padding: 12px var(--row-x) 0; font-size: var(--text-sm); font-weight: 600; color: var(--ink-2); }
  .subhead + :global(.tiles) { border-top: 0; }
  :global(.tiles + .subhead) { border-top: 1px solid var(--line); }
  .pick { display: flex; justify-content: flex-end; margin-top: var(--gap-group); }
  .help { margin: 0; padding: 14px var(--row-x); }
</style>
