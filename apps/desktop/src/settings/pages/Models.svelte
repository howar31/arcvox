<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { errorMessage, formatBytes, modelsCancel, modelsDelete, modelsDir, modelsDownload, modelsDownloadRecommended, modelsImport, modelsInstalled, modelsManifest, modelsManifestNow, onModelDownloadProgress, reveal, type DownloadProgress, type InstalledModel, type ManifestView } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { rowStatus } from "../../lib/models.svelte";

  let manifest = $state<ManifestView | null>(null);
  let installed = $state<InstalledModel[]>([]);
  let dir = $state("");
  let progress = $state<Record<string, DownloadProgress>>({});
  let status = $state("");

  const busy = $derived(Object.values(progress).some((p) => !p.done));

  async function refreshInstalled() {
    try { installed = await modelsInstalled(); } catch (e) { status = errorMessage(e); }
  }
  async function refreshManifest() {
    try {
      // Render the local catalogue first; the online one can take until its fetch times out.
      if (!manifest) manifest = await modelsManifestNow();
      manifest = await modelsManifest();
    } catch (e) { status = errorMessage(e); }
  }
  onMount(() => {
    refreshManifest();
    refreshInstalled();
    modelsDir().then((d) => { dir = d; }).catch((e) => { status = errorMessage(e); });
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onModelDownloadProgress((p) => {
      progress = { ...progress, [p.id]: p };
      if (p.done && !p.error) refreshInstalled();
    }).then((u) => { if (disposed) u(); else unlisten = u; });
    return () => { disposed = true; unlisten?.(); };
  });

  function download(id: string) {
    status = "";
    modelsDownload(id).catch((e) => { status = errorMessage(e); });
  }
  function downloadRecommended() {
    status = "";
    modelsDownloadRecommended().catch((e) => { status = errorMessage(e); });
  }
  async function cancel(id: string) {
    try { await modelsCancel(id); } catch (e) { status = errorMessage(e); }
  }
  async function remove(id: string) {
    try { await modelsDelete(id); } catch (e) { status = errorMessage(e); }
    await refreshInstalled();
  }
  async function importFile() {
    try {
      const path = await open({ multiple: false, directory: false, filters: [{ name: "Model", extensions: ["bin", "gguf"] }] });
      if (!path) return;
      await modelsImport(path);
      status = t("models.imported");
      await refreshInstalled();
    } catch (e) { status = errorMessage(e); }
  }
  function percent(received: number, total: number): string {
    return total > 0 ? `${Math.min(100, Math.floor((received / total) * 100))}%` : "";
  }
</script>

<h2>{t("models.title")}</h2>
<p class="hint">{t("models.hint_profile")}</p>
<div class="row">
  {#if manifest}<span class="muted">{t(`models.source.${manifest.source}`)}</span>{/if}
  <button onclick={refreshManifest}>{t("models.refresh")}</button>
  <button class="primary" disabled={busy} onclick={downloadRecommended}>{t("models.download_recommended")}</button>
</div>
<table>
  <thead><tr><th>{t("models.col.model")}</th><th>{t("models.col.languages")}</th><th>{t("models.col.size")}</th><th>{t("models.col.license")}</th><th>{t("models.col.status")}</th></tr></thead>
  <tbody>
    {#each manifest?.models ?? [] as m (m.id)}
      {@const s = rowStatus(m, installed, progress)}
      <tr>
        <td>{m.label}<div class="muted">{m.description}</div></td>
        <td>{m.languages.join(", ")}</td>
        <td>{formatBytes(m.size_bytes)}</td>
        <td>{m.license}</td>
        <td>
          {#if s.kind === "missing"}
            <button onclick={() => download(m.id)}>{t("models.download")}</button>
          {:else if s.kind === "downloading"}
            <span>{t("models.status.downloading")} {percent(s.received, s.total)}</span>
            <button onclick={() => cancel(m.id)}>{t("models.cancel")}</button>
          {:else if s.kind === "installed"}
            <span>{t("models.status.installed")} {formatBytes(s.size)}</span>
            <button onclick={() => remove(m.id)}>{t("models.delete")}</button>
          {:else}
            <span class="muted">{s.message}</span>
            <button onclick={() => download(m.id)}>{t("models.retry")}</button>
          {/if}
        </td>
      </tr>
    {/each}
  </tbody>
</table>

<h3>{t("models.custom")}</h3>
<table>
  <tbody>
    {#each installed.filter((m) => m.custom) as m (m.id)}
      <tr>
        <td>{m.id}</td><td>{formatBytes(m.size_bytes)}</td>
        <td><button onclick={() => remove(m.id)}>{t("models.delete")}</button></td>
      </tr>
    {/each}
  </tbody>
</table>
<button onclick={importFile}>{t("models.import")}</button>

<div class="row">
  <span class="muted">{t("models.directory")}: {dir}</span>
  <button onclick={() => reveal(dir)} disabled={!dir}>{t("models.reveal")}</button>
</div>
<p class="status">{status}</p>
