<script lang="ts">
  import { tick } from "svelte";
  import { appName } from "../../../lib/apps.svelte";
  import { t } from "../../../lib/i18n";
  import { isImeKey } from "../../../lib/ime";
  import { tip } from "../../../lib/tip";
  import AddRow from "../../../lib/ui/AddRow.svelte";
  import AppIcon from "../../../lib/ui/AppIcon.svelte";
  import Fold from "../../../lib/ui/Fold.svelte";
  import Icon from "../../../lib/ui/Icon.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";
  import AddPerApp from "../AddPerApp.svelte";

  /** The applications and web addresses a profile is used in, in one list. */
  let { apps, urls, url = $bindable(""), onaddapp, onaddurl, onremoveapp, onremoveurl }: {
    apps: string[];
    urls: string[];
    /** The address typed and not added yet. */
    url?: string;
    onaddapp: (id: string) => void;
    onaddurl: () => void;
    onremoveapp: (index: number) => void;
    onremoveurl: (index: number) => void;
  } = $props();

  let adding = $state(false);
  let field: HTMLInputElement | undefined = $state();

  async function showField() {
    adding = true;
    await tick();
    field?.focus();
  }
  function hideField() {
    adding = false;
    url = "";
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !isImeKey(e)) onaddurl();
    else if (e.key === "Escape" && !isImeKey(e)) { hideField(); e.stopPropagation(); }
  }
</script>

{#each apps as id, i (id)}
  <div class="item">
    <AppIcon bundleId={id} size={20} />
    <span class="name" use:tip={id}>{appName(id)}</span>
    <Tag>{t("profiles.apps_label")}</Tag>
    <button type="button" class="remove" aria-label={t("common.remove")} onclick={() => onremoveapp(i)}><Icon name="x" size={12} /></button>
  </div>
{/each}
{#each urls as u, i (u)}
  <div class="item">
    <span class="globe"><Icon name="globe" size={16} /></span>
    <span class="name">{u}</span>
    <Tag title={t("profiles.urls_help")}>{t("profiles.urls")}</Tag>
    <button type="button" class="remove" aria-label={t("common.remove")} onclick={() => onremoveurl(i)}><Icon name="x" size={12} /></button>
  </div>
{/each}
{#if apps.length === 0 && urls.length === 0}
  <p class="none">{t("flow.applies_none")}</p>
{/if}
<AddRow>
  <AddPerApp bare onadd={onaddapp} exclude={apps} />
  <button type="button" class="btn" aria-expanded={adding} onclick={() => (adding ? hideField() : showField())}><Icon name="plus" size={14} />{t("flow.add_url")}</button>
</AddRow>
{#if adding}<Fold>
  <div class="field">
    <input type="text" bind:this={field} bind:value={url} placeholder={t("profiles.url_placeholder")} aria-label={t("profiles.urls")} {onkeydown} />
    <button type="button" class="btn" disabled={!url.trim()} onclick={onaddurl}>{t("common.add")}</button>
  </div>
</Fold>{/if}

<style>
  .item { display: flex; align-items: center; gap: 12px; min-height: 52px; padding: 10px 12px 10px var(--row-x); color: var(--ink); }
  .item + .item { border-top: 1px solid var(--line); }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--text); }
  .globe { flex: none; display: grid; place-items: center; width: 20px; height: 20px; color: var(--ink-2); }
  .remove { flex: none; display: grid; place-items: center; width: 28px; height: 28px; margin-left: auto; padding: 0; border: 0; border-radius: 8px; background: none; color: var(--ink-3); cursor: default; }
  .remove:hover { background: var(--sunken); color: var(--ink); }
  .none { margin: 0; padding: 16px var(--row-x); color: var(--ink-2); font-size: var(--text-sm); }
  .field { display: flex; gap: 10px; padding: 0 var(--row-x) 14px; }
  .field input { flex: 1; min-width: 0; }
</style>
