<script lang="ts">
  import { onMount } from "svelte";
  import { runningApps, type RunningApp } from "../../lib/api";
  import { t } from "../../lib/i18n";
  let { onadd }: { onadd: (id: string) => void } = $props();
  let apps = $state<RunningApp[]>([]);
  let chosen = $state("");
  onMount(async () => { apps = await runningApps(); });
</script>
<div class="row">
  <select bind:value={chosen}>
    <option value="">{t("general.pick_app")}</option>
    {#each apps as a}<option value={a.bundle_id}>{a.name} ({a.bundle_id})</option>{/each}
  </select>
  <input placeholder="com.example.app" bind:value={chosen} />
  <button onclick={() => { if (chosen) { onadd(chosen); chosen = ""; } }}>{t("common.add")}</button>
</div>
