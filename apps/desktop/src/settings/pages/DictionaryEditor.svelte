<script lang="ts">
  /** The one editor of a dictionary entry, shown under the chosen row or card or in the right pane: the term,
   * the aliases, the recognition hint, where the entry came from, the sentences it appeared in, and removal. */
  import { tip } from "../../lib/tip";
  import type { DictionaryEntry, TermHits } from "../../lib/api";
  import { addAliases, isSnippet } from "../../lib/dictionary";
  import { formatShortDate } from "../../lib/format";
  import { isImeKey } from "../../lib/ime";
  import { t } from "../../lib/i18n";
  import Chip from "../../lib/ui/Chip.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import Tag from "../../lib/ui/Tag.svelte";

  type Item = DictionaryEntry & { key: number; pending: string };
  let { item = $bindable(), examples, hits = null, invalid = false, onremove, autofocus = false }: {
    item: Item;
    /** Sentences from the history that hold the term; null while they load. */
    examples: string[] | null;
    /** How often the term's aliases were replaced, when ever. */
    hits?: TermHits | null;
    invalid?: boolean;
    onremove: () => void;
    autofocus?: boolean;
  } = $props();

  // A first press asks; the second within a few seconds removes.
  let sure = $state(false);
  let sureTimer: ReturnType<typeof setTimeout> | undefined;
  function remove() {
    if (!sure) {
      sure = true;
      clearTimeout(sureTimer);
      sureTimer = setTimeout(() => { sure = false; }, 4000);
      return;
    }
    clearTimeout(sureTimer);
    onremove();
  }

  function commitAlias() {
    if (!item.pending.trim()) { item.pending = ""; return; }
    item.aliases = addAliases(item.aliases, item.pending);
    item.pending = "";
  }
  function aliasKey(e: KeyboardEvent) {
    if (isImeKey(e)) return;
    if (e.key === "Enter" || e.key === "," || e.key === "，") {
      e.preventDefault();
      commitAlias();
    } else if (e.key === "Backspace" && item.pending === "" && item.aliases.length) {
      e.preventDefault();
      item.aliases = item.aliases.slice(0, -1);
    }
  }

  /** Grows a text box with its content, from one line up. */
  function grow(el: HTMLTextAreaElement, _value: string) {
    const fit = () => {
      el.style.height = "auto";
      el.style.height = `${el.scrollHeight + el.offsetHeight - el.clientHeight}px`;
    };
    fit();
    el.addEventListener("input", fit);
    if (autofocus) el.focus();
    return { update: fit, destroy: () => el.removeEventListener("input", fit) };
  }

  /** The excerpt's pieces: the term is between 【 and 】. */
  function pieces(excerpt: string): { text: string; mark: boolean }[] {
    return excerpt.split(/(【[^】]*】)/).filter(Boolean).map((p) => (p.startsWith("【") ? { text: p.slice(1, -1), mark: true } : { text: p, mark: false }));
  }

  const origin = $derived.by(() => {
    const sep = t("common.list_sep") === "、" ? "，" : ", ";
    const parts: string[] = [];
    if (item.origin || item.added) {
      const what = t(item.origin === "suggestion" ? "dictionary.origin_suggestion" : "dictionary.origin_manual");
      parts.push(item.added ? t("dictionary.origin_on", { origin: what, date: formatShortDate(item.added) }) : what);
      const heard = (item.heard ?? []).filter((h) => h.trim());
      if (heard.length) parts.push(t("dictionary.heard_before", { list: heard.join(t("common.list_sep")) }));
      if (item.updated && item.updated !== item.added) parts.push(t("dictionary.updated_on", { date: formatShortDate(item.updated) }));
    }
    if (hits) parts.push(t("dictionary.hits_line", { n: String(hits.count), date: formatShortDate(hits.last) }));
    return parts.join(sep);
  });
</script>

<div class="editor">
  <div class="line">
    <textarea class="term" rows="1" bind:value={item.term} use:grow={item.term} placeholder={t("dictionary.term")} aria-label={t("dictionary.term")} aria-invalid={invalid ? "true" : undefined}></textarea>
    {#if isSnippet(item.term)}<span class="tag"><Tag tone="accent">{t("dictionary.snippet")}</Tag></span>{/if}
  </div>
  <div class="line aliases-line">
    <span class="sub lead">{t("dictionary.aliases")}</span>
    <div class="aliases">
      {#each item.aliases as a, j (j)}
        <Chip onremove={() => item.aliases.splice(j, 1)}>{a}</Chip>
      {/each}
      <input type="text" class="alias-input" bind:value={item.pending} placeholder={t("dictionary.aliases_placeholder")} aria-label={t("dictionary.aliases")} onkeydown={aliasKey} onblur={commitAlias} />
    </div>
  </div>
  <div class="line foot">
    <span class="hint"><Switch bind:checked={item.asr_hint} label={t("dictionary.asr_hint")} /><span class="sub">{t("dictionary.asr_hint")}</span></span>
    <span class="origin" use:tip={origin}>{origin}</span>
    {#if sure}
      <button type="button" class="btn danger" onclick={remove}>{t("dictionary.remove_sure")}</button>
    {:else}
      <button type="button" class="btn quiet danger" use:tip={t("common.remove")} onclick={remove}>{t("common.remove")}</button>
    {/if}
  </div>
  {#if examples && examples.length}
    <div class="examples">
      <div class="label">{t("dictionary.examples")}</div>
      {#each examples as ex, i (i)}<div class="excerpt">{#each pieces(ex) as p, j (j)}{#if p.mark}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}</div>{/each}
    </div>
  {/if}
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .line { display: flex; align-items: flex-start; gap: 10px; min-width: 0; }
  .aliases-line { align-items: center; flex-wrap: wrap; }
  .foot { align-items: center; }
  .term { flex: 1; min-width: 0; min-height: 0; height: var(--control); padding: 5px 10px; border-radius: var(--radius); line-height: 20px; font-size: var(--text-control); resize: none; overflow: hidden; }
  .tag { flex: none; margin-top: 5px; }
  .sub { color: var(--ink-2); font-size: var(--text-sm); }
  .lead { flex: none; }
  .aliases { flex: 1; min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .alias-input { flex: 1; min-width: 90px; height: 28px; padding: 0 4px; border: 0; background: none; font-size: var(--text-sm); }
  .hint { display: inline-flex; align-items: center; gap: 8px; flex: none; }
  /* Takes the room between the switch and the button; a long line is cut, the tooltip holds it whole. */
  .origin { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-3); font-size: var(--text-sm); }
  .foot .btn:last-child { margin-right: -8px; }
  .examples { display: flex; flex-direction: column; gap: 4px; }
  .label { font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); }
  .excerpt { color: var(--ink-2); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .excerpt mark { background: var(--accent-soft); color: var(--ink); border-radius: 4px; padding: 0 2px; }
</style>
