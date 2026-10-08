<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, openLink, reveal, systemInfo, type AboutLink, type SystemInfo } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { go } from "../../lib/route";
  import { toast } from "../../lib/ui/toast.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Icon, { type IconName } from "../../lib/ui/Icon.svelte";
  import Mark from "../../lib/ui/Mark.svelte";
  import Tag from "../../lib/ui/Tag.svelte";

  let info = $state<SystemInfo | null>(null);
  const links: { link: AboutLink; icon: IconName; label: string }[] = [
    { link: "sponsor", icon: "heart", label: "about.sponsor" },
    { link: "source", icon: "code", label: "about.source" },
    { link: "issue", icon: "bug", label: "about.issue" },
    { link: "website", icon: "about", label: "about.website" },
  ];

  onMount(async () => {
    try { info = await systemInfo(); } catch (e) { toast.error(errorMessage(e)); }
  });

  async function open(link: AboutLink) {
    try { await openLink(link); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function show(path: string) {
    try { await reveal(path); } catch (e) { toast.error(errorMessage(e)); }
  }
</script>

<div class="lockup">
  <Mark size={64} />
  <div class="word"><em>A</em>rcvox</div>
  {#if info}
    <div class="version"><span>{t("about.version", { version: info.version })}</span>{#if info.dev}<Tag>{t("about.dev")}</Tag>{/if}</div>
  {/if}
</div>

<Group title={t("about.links")}>
  {#each links as l (l.link)}
    <Row label={t(l.label)} onclick={() => open(l.link)} external>
      {#snippet leading()}<span class="lead"><Icon name={l.icon} size={18} /></span>{/snippet}
    </Row>
  {/each}
  <Row label={t("about.licenses")} onclick={() => go("licenses")}>
    {#snippet leading()}<span class="lead"><Icon name="scale" size={18} /></span>{/snippet}
  </Row>
</Group>

{#if info}
  <Group title={t("about.locations")} help={t("about.model_licenses")}>
    {#each [["about.config_dir", info.config_dir], ["about.data_dir", info.data_dir], ["about.recordings_dir", info.recordings_dir], ["about.cache_dir", info.cache_dir]] as [key, path] (key)}
      <Row label={t(key)} help={path}>
        <button type="button" class="btn quiet" onclick={() => show(path)}><Icon name="folder" size={14} />{t("about.reveal")}</button>
      </Row>
    {/each}
  </Group>
{/if}

<footer class="foot">{t("about.copyright")}<br>{t("about.license")}</footer>

<style>
  /* The lockup takes the page head's place; the update state will sit under the version. */
  .lockup { display: flex; flex-direction: column; align-items: center; text-align: center; padding: 8px 0 0; }
  .word { margin-top: 14px; font: 800 30px/1 var(--font-round); letter-spacing: -.035em; }
  .word em { font-style: normal; background: linear-gradient(315deg, var(--brand-1), var(--brand-2) 52%, var(--brand-3)); -webkit-background-clip: text; background-clip: text; color: transparent; }
  .version { display: flex; align-items: center; gap: 8px; margin-top: 10px; min-height: 24px; color: var(--ink-2); font-size: var(--text-control); }
  .lead { display: flex; color: var(--accent); }
  .foot { margin-top: 40px; text-align: center; color: var(--ink-3); font-size: var(--text-xs); line-height: 1.7; }
</style>
