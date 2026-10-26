<script lang="ts">
  /** Report an issue: the bug form on GitHub with the snapshot filled in, the snapshot itself, and the places the app keeps files in. */
  import { onMount } from "svelte";
  import { diagnostics, errorMessage, openIssue, reveal, widgetCopy, type Diagnostics, type Location } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { toast } from "../../lib/ui/toast.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import PageLink from "../../lib/ui/PageLink.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Tag from "../../lib/ui/Tag.svelte";

  let report = $state<Diagnostics | null>(null);
  // Where the settings and the records share a folder, the app folder holds both.
  const split = $derived(report?.locations.some((l) => l.kind === "data") ?? false);

  onMount(() => {
    diagnostics().then((d) => { report = d; }).catch((e) => toast.error(errorMessage(e)));
  });

  async function open() {
    if (!report) return;
    try { await openIssue(report.issue); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function copy() {
    if (!report) return;
    try { await widgetCopy(report.text); toast.ok(t("report.copied")); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function show(path: string) {
    try { await reveal(path); } catch (e) { toast.error(errorMessage(e)); }
  }

  function info(l: Location): string | undefined {
    switch (l.kind) {
      case "app": return t(split ? "report.app_info_split" : "report.app_info");
      case "data": return t("report.data_info");
      case "cache": return t("report.cache_info");
      default: return undefined;
    }
  }
</script>

{#snippet logInfo()}{t("report.log_info_before")}<PageLink page="general" label={t("nav.general")} />{t("report.log_info_after")}{/snippet}

<PageHead title={t("about.issue")} description={t("report.description")} crumb={{ label: t("nav.about"), page: "about" }} />

<div class="actions">
  <button type="button" class="btn primary" disabled={!report} onclick={open}><Icon name="external" size={14} />{t("report.open")}</button>
  <button type="button" class="btn" disabled={!report} onclick={copy}><Icon name="copy" size={14} />{t("report.copy")}</button>
</div>

<Group title={t("report.diagnostics")} info={t("report.diagnostics_info")}>
  <pre class="diag" class:waiting={!report}>{report?.text ?? t("report.collecting")}</pre>
</Group>

{#if report}
  <Group title={t("report.locations")}>
    {#each report.locations as l (l.kind)}
      <Row label={t("report.loc_" + l.kind)} help={l.path} info={l.kind === "log" ? logInfo : info(l)}>
        {#snippet tag()}
          {#if !l.exists}<Tag>{t(l.kind === "backup" || l.kind === "log" ? "report.not_made" : "report.missing")}</Tag>
          {:else if l.is_default}<Tag>{t("report.default")}</Tag>{/if}
        {/snippet}
        {#if l.exists}
          <button type="button" class="btn quiet" onclick={() => show(l.path)}><Icon name="folder" size={14} />{t("report.reveal")}</button>
        {/if}
      </Row>
    {/each}
  </Group>
{/if}

<style>
  .actions { display: flex; gap: 10px; margin-bottom: 4px; }
  .diag { margin: 0; padding: 14px var(--row-x); font-family: var(--font-mono); font-size: var(--text-xs); line-height: 1.6; color: var(--ink); white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; -webkit-user-select: text; cursor: text; }
  .diag.waiting { color: var(--ink-3); }
</style>
