<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, setDictionary, type DictionaryEntry } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { config } from "../../lib/store.svelte";

  let entries = $state<DictionaryEntry[]>([]);
  let status = $state("");
  let term = $state("");
  let aliases = $state("");
  onMount(() => { if (config.value) entries = structuredClone($state.snapshot(config.value.dictionary)); });

  function add() {
    if (!term.trim()) return;
    entries.push({ term: term.trim(), aliases: aliases.split(/[,，\s]+/).map((a) => a.trim()).filter(Boolean), asr_hint: true });
    term = ""; aliases = "";
  }
  // Grow with the number of lines, up to six.
  function fit(ev: Event) {
    const el = ev.currentTarget as HTMLTextAreaElement;
    el.rows = Math.min(6, el.value.split("\n").length);
  }
  async function save() {
    try {
      const result = await setDictionary(entries);
      entries = structuredClone($state.snapshot(result.dictionary));
      status = t("common.saved");
    } catch (e) { status = errorMessage(e); }
  }
</script>

<h2>{t("nav.dictionary")}</h2>
<p class="muted">{t("dictionary.note")}</p>
<table>
  <thead><tr><th>{t("dictionary.term")}</th><th>{t("dictionary.aliases")}</th><th>{t("dictionary.asr_hint")}</th><th></th></tr></thead>
  <tbody>
    {#each entries as e, i}
      <tr>
        <td><textarea rows={Math.min(6, e.term.split("\n").length)} bind:value={e.term} oninput={fit}></textarea></td>
        <td><input value={e.aliases.join(", ")} onchange={(ev) => { e.aliases = (ev.currentTarget as HTMLInputElement).value.split(/[,，\s]+/).map((a) => a.trim()).filter(Boolean); }} /></td>
        <td><input type="checkbox" bind:checked={e.asr_hint} /></td>
        <td><button onclick={() => entries.splice(i, 1)}>{t("common.remove")}</button></td>
      </tr>
    {/each}
  </tbody>
</table>
<div class="row">
  <textarea rows="1" placeholder={t("dictionary.term")} bind:value={term} oninput={fit}></textarea>
  <input placeholder={t("dictionary.aliases_placeholder")} bind:value={aliases} />
  <button onclick={add}>{t("common.add")}</button>
</div>
<div class="row"><button class="primary" onclick={save}>{t("common.save")}</button><span class="status">{status}</span></div>

<style>
  textarea { white-space: pre; resize: vertical; }
</style>
