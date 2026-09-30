<script lang="ts">
  import { onMount } from "svelte";
  import { runningApps, type RunningApp } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import Row from "../../lib/ui/Row.svelte";
  import Icon from "../../lib/ui/Icon.svelte";

  let { onadd }: { onadd: (id: string) => void } = $props();
  let apps = $state<RunningApp[]>([]);
  let chosen = $state("");
  onMount(async () => { apps = await runningApps(); });

  function add() {
    const id = chosen.trim();
    if (!id) return;
    onadd(id);
    chosen = "";
  }
</script>

<Row>
  <select class="apps" bind:value={chosen} aria-label={t("general.pick_app")}>
    <option value="">{t("general.pick_app")}</option>
    {#each apps as a (a.bundle_id)}<option value={a.bundle_id}>{a.name}</option>{/each}
  </select>
  <input class="bundle" type="text" placeholder="com.example.app" aria-label={t("general.bundle_id")} bind:value={chosen} onkeydown={(e) => { if (e.key === "Enter") add(); }} />
  <button type="button" class="btn" disabled={!chosen.trim()} onclick={add}><Icon name="plus" size={14} />{t("common.add")}</button>
</Row>

<style>
  .apps { width: 150px; flex: none; }
  .bundle { flex: 1; min-width: 0; }
</style>
