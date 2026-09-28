<script lang="ts">
  import { onMount } from "svelte";
  import { clearHistory, deleteHistory, errorMessage, listHistory, widgetCopy, type Utterance } from "../../lib/api";
  import { t } from "../../lib/i18n";

  let rows = $state<Utterance[]>([]);
  let status = $state("");
  async function reload() { try { rows = await listHistory(200); } catch (e) { status = errorMessage(e); } }
  onMount(reload);
  function when(ts: number) { return new Date(ts * 1000).toLocaleString(); }
  async function remove(id: number) { try { await deleteHistory(id); await reload(); } catch (e) { status = errorMessage(e); } }
  async function clearAll() { if (confirm(t("history.confirm_clear"))) { try { await clearHistory(); await reload(); } catch (e) { status = errorMessage(e); } } }
</script>

<h2>{t("nav.history")}</h2>
<div class="row"><button onclick={reload}>{t("history.reload")}</button><button onclick={clearAll}>{t("history.clear_all")}</button><span class="status">{status}</span></div>
<table>
  <thead><tr><th>{t("history.time")}</th><th>{t("history.app")}</th><th>{t("history.raw")}</th><th>{t("history.final")}</th><th></th></tr></thead>
  <tbody>
    {#each rows as u}
      <tr class:degraded={u.degraded}>
        <td>{when(u.ts)}</td><td>{u.app_name ?? ""}</td><td>{u.raw_text}</td><td>{u.final_text}</td>
        <td><button onclick={() => widgetCopy(u.final_text)}>{t("widget.copy")}</button> <button onclick={() => remove(u.id)}>{t("common.remove")}</button></td>
      </tr>
    {/each}
  </tbody>
</table>

<style>
  tr.degraded td { color: var(--muted); }
</style>
