<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, getPrompt, restorePrompt, setPrompt } from "../../lib/api";
  import { t } from "../../lib/i18n";
  let text = $state("");
  let status = $state("");
  onMount(async () => { text = await getPrompt(); });
  async function save() { try { await setPrompt(text); status = t("common.saved"); } catch (e) { status = errorMessage(e); } }
  async function restore() { try { text = await restorePrompt(); status = t("prompt.restored"); } catch (e) { status = errorMessage(e); } }
</script>
<h2>{t("nav.prompt")}</h2>
<p class="muted">{t("prompt.note")}</p>
<textarea rows="18" bind:value={text}></textarea>
<div class="row">
  <button class="primary" onclick={save}>{t("common.save")}</button>
  <button onclick={restore}>{t("prompt.restore")}</button>
  <span class="status">{status}</span>
</div>
