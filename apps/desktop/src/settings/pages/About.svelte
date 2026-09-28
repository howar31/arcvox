<script lang="ts">
  import { onMount } from "svelte";
  import { reveal, systemInfo, type SystemInfo } from "../../lib/api";
  import { t } from "../../lib/i18n";
  let info = $state<SystemInfo | null>(null);
  onMount(async () => { info = await systemInfo(); });
</script>
<h2>{t("nav.about")}</h2>
{#if info}
  <p>Arcvox {info.version}</p>
  <p>{t("about.license")}</p>
  <dl>
    <dt>{t("about.config_dir")}</dt><dd><code>{info.config_dir}</code> <button onclick={() => reveal(info!.config_dir)}>{t("about.reveal")}</button></dd>
    <dt>{t("about.data_dir")}</dt><dd><code>{info.data_dir}</code> <button onclick={() => reveal(info!.data_dir)}>{t("about.reveal")}</button></dd>
    <dt>{t("about.cache_dir")}</dt><dd><code>{info.cache_dir}</code> <button onclick={() => reveal(info!.cache_dir)}>{t("about.reveal")}</button></dd>
  </dl>
{/if}
