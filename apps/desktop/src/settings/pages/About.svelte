<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, onUpdateStatus, openLink, systemInfo, updateCheck, updateInstall, updateReleases, updateStatus, type AboutLink, type Release, type SystemInfo, type UpdateStatus } from "../../lib/api";
  import { currentLanguage, t } from "../../lib/i18n";
  import { sections, title } from "../../lib/whatsNew";
  import { tip } from "../../lib/tip";
  import { go } from "../../lib/route";
  import { guide } from "../../lib/guide.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Icon, { type IconName } from "../../lib/ui/Icon.svelte";
  import Mark from "../../lib/ui/Mark.svelte";
  import Tag from "../../lib/ui/Tag.svelte";

  let info = $state<SystemInfo | null>(null);
  let update = $state<UpdateStatus>({ state: "idle" });
  let releases = $state<Release[]>([]);
  const whatsNew = $derived(title(sections(releases, currentLanguage()), t));
  const percent = $derived(update.state === "installing" && update.total ? Math.min(100, Math.round((update.received / update.total) * 100)) : 0);
  const links: { link: AboutLink; icon: IconName; label: string }[] = [
    { link: "sponsor", icon: "heart", label: "about.sponsor" },
    { link: "source", icon: "code", label: "about.source" },
    { link: "website", icon: "about", label: "about.website" },
  ];

  onMount(() => {
    systemInfo().then((i) => { info = i; }).catch((e) => toast.error(errorMessage(e)));
    updateStatus().then((s) => { update = s; }).catch(() => {});
    const loadReleases = () => updateReleases().then((r) => { releases = r; }).catch(() => {});
    loadReleases();
    // The releases are read before a state is announced.
    const off = onUpdateStatus((s) => { update = s; loadReleases(); });
    return () => { off.then((f) => f()); };
  });

  async function check() {
    try { update = await updateCheck(); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function install() {
    try { await updateInstall(); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function open(link: AboutLink) {
    try { await openLink(link); } catch (e) { toast.error(errorMessage(e)); }
  }

</script>

<div class="lockup">
  <Mark size={64} />
  <div class="word"><em>A</em>rcvox</div>
  {#if info}
    <div class="version">
      <span>{t("about.version", { version: info.version })}</span>
      {#if info.dev}<Tag>{t("about.dev")}</Tag>{/if}
      {#if update.state !== "installing"}
        <button type="button" class="icon-btn" class:spin={update.state === "checking"} onclick={check} disabled={update.state === "checking"} aria-label={t("about.update.check")} use:tip={t("about.update.check")}><Icon name="refresh" size={14} /></button>
      {/if}
    </div>
    <!-- The update state sits under the version, where the eye already is. -->
    <div class="update">
      {#if update.state === "checking"}<span>{t("about.update.checking")}</span>
      {:else if update.state === "current"}<span><Icon name="check" size={14} />{t("about.update.current")}</span>
      {:else if update.state === "failed"}<span class="bad" use:tip={update.message}>{t("about.update.failed")}</span>
      {:else if update.state === "available"}
        <span class="new">{t("about.update.available", { version: update.version })}</span>
        {#if update.installable}
          <button type="button" class="btn" onclick={install}><Icon name="download" size={14} />{t("about.update.install")}</button>
        {:else}
          <button type="button" class="btn" onclick={() => open("releases")}><Icon name="external" size={14} />{t("about.update.download")}</button>
        {/if}
      {:else if update.state === "installing"}
        <span>{t("about.update.installing", { version: update.version })}</span>
        <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} aria-label={t("about.update.installing", { version: update.version })}><i style:width="{percent}%"></i></div>
        <span class="num">{percent}%</span>
      {/if}
    </div>
  {/if}
</div>

{#if whatsNew}
  <Group>
    <Row label={whatsNew} onclick={() => go("whatsnew")}>
      {#snippet leading()}<span class="lead"><Icon name="sparkle" size={18} /></span>{/snippet}
    </Row>
  </Group>
{/if}

<Group title={t("about.links")}>
  {#each links as l (l.link)}
    <Row label={t(l.label)} onclick={() => open(l.link)} external>
      {#snippet leading()}<span class="lead"><Icon name={l.icon} size={18} /></span>{/snippet}
    </Row>
  {/each}
  <Row label={t("about.issue")} onclick={() => go("report")}>
    {#snippet leading()}<span class="lead"><Icon name="bug" size={18} /></span>{/snippet}
  </Row>
  <Row label={t("about.licenses")} onclick={() => go("licenses")}>
    {#snippet leading()}<span class="lead"><Icon name="scale" size={18} /></span>{/snippet}
  </Row>
</Group>

<div class="again"><button type="button" onclick={() => { guide.reset(); go("welcome"); }}>{t("about.guide_again")}</button></div>

<footer class="foot">{t("about.copyright")}<br>{t("about.license")}</footer>

<style>
  /* The lockup takes the page head's place; the update state sits under the version. */
  .lockup { display: flex; flex-direction: column; align-items: center; text-align: center; padding: 8px 0 0; }
  .word { margin-top: 14px; font: 800 30px/1 var(--font-round); letter-spacing: -.035em; }
  .word em { font-style: normal; background: linear-gradient(315deg, var(--brand-1), var(--brand-2) 52%, var(--brand-3)); -webkit-background-clip: text; background-clip: text; color: transparent; }
  .version { display: flex; align-items: center; gap: 8px; margin-top: 10px; min-height: 24px; color: var(--ink-2); font-size: var(--text-control); }
  .lead { display: flex; color: var(--accent); }
  .icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: 6px; background: none; color: var(--ink-3); cursor: pointer; }
  .icon-btn:hover:not(:disabled) { background: var(--sunken); color: var(--ink); }
  .icon-btn.spin :global(svg) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .icon-btn.spin :global(svg) { animation: none; } }
  .update { display: flex; align-items: center; justify-content: center; gap: 10px; min-height: 30px; margin-top: 6px; color: var(--ink-3); font-size: var(--text-sm); }
  .update span { display: inline-flex; align-items: center; gap: 4px; }
  .update .new { color: var(--accent); font-weight: 600; }
  .update .bad { color: var(--live); }
  .update .bar { width: 160px; height: 4px; border-radius: 2px; background: var(--line); overflow: hidden; }
  .update .bar i { display: block; height: 100%; background: var(--accent); }
  .again { display: flex; justify-content: center; margin-top: 18px; }
  .again button { padding: 4px 8px; border: 0; border-radius: 6px; background: none; color: var(--ink-3); font-size: var(--text-sm); cursor: default; }
  .again button:hover { background: var(--sunken); color: var(--ink-2); }
  .foot { margin-top: 18px; text-align: center; color: var(--ink-3); font-size: var(--text-xs); line-height: 1.7; }
</style>
