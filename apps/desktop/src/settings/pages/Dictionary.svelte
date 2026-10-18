<script lang="ts">
  import { tip } from "../../lib/tip";
  import { onMount, tick, untrack } from "svelte";
  import { dictionarySuggestions, errorMessage, setDictionary, type DictionaryEntry, type Suggestion } from "../../lib/api";
  import { addAliases, dropBlank, isSnippet, trimEntries } from "../../lib/dictionary";
  import { guard } from "../../lib/guard.svelte";
  import { isImeKey } from "../../lib/ime";
  import { t } from "../../lib/i18n";
  import { config } from "../../lib/store.svelte";
  import Chip from "../../lib/ui/Chip.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Fold from "../../lib/ui/Fold.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import InfoTip from "../../lib/ui/InfoTip.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  /** An entry being edited: `key` keeps each row's elements stable, `pending` is the alias being typed. */
  type Item = DictionaryEntry & { key: number; pending: string };

  let nextKey = 0;
  const toItems = (entries: DictionaryEntry[]): Item[] =>
    entries.map((e) => ({ ...structuredClone(e), key: nextKey++, pending: "" }));
  const toEntries = (items: Item[]): DictionaryEntry[] =>
    items.map(({ term, aliases, asr_hint }) => ({ term, aliases: [...aliases], asr_hint }));

  const stored = $derived(config.value?.dictionary ?? null);
  let items = $state<Item[] | null>(null);
  // The stored dictionary the items were last copied from.
  let synced = "";
  let saving = $state(false);
  let problem = $state<string | undefined>(undefined);
  let list: HTMLElement | undefined = $state();

  // Copy the stored dictionary in, and again when it changes while nothing is edited.
  $effect(() => {
    if (!stored) return;
    const next = JSON.stringify(stored);
    untrack(() => {
      if (items === null || (JSON.stringify(toEntries(items)) === synced && !items.some((item) => item.pending.trim()))) {
        items = toItems($state.snapshot(stored) as DictionaryEntry[]);
        synced = next;
      }
    });
  });

  // Suggestions: terms the dictations the user corrected were heard as something else.
  const OPEN_KEY = "dictionary.suggestions.open";
  let suggestions = $state<Suggestion[] | null>(null);
  let suggestionsOpen = $state(storedOpen());
  let showAll = $state(false);
  // Acted on in this draft: taken into the list, or turned down. Saved with the list.
  let taken = $state<string[]>([]);
  let turnedDown = $state<string[]>([]);
  const visible = $derived((suggestions ?? []).filter((s) => !taken.includes(s.term) && !turnedDown.includes(s.term)));
  const shown = $derived(showAll ? visible : visible.slice(0, 4));

  function storedOpen(): boolean {
    try { return localStorage.getItem(OPEN_KEY) !== "0"; } catch { return true; }
  }
  function toggleSuggestions() {
    suggestionsOpen = !suggestionsOpen;
    try { localStorage.setItem(OPEN_KEY, suggestionsOpen ? "1" : "0"); } catch { /* The state is not remembered. */ }
  }
  async function loadSuggestions() {
    try { suggestions = await dictionarySuggestions(); } catch (e) { suggestions = []; toast.error(errorMessage(e)); }
  }
  onMount(() => { void loadSuggestions(); });

  /** The excerpt's pieces: the term is between 【 and 】. */
  function pieces(excerpt: string): { text: string; mark: boolean }[] {
    return excerpt.split(/(【[^】]*】)/).filter(Boolean).map((p) => (p.startsWith("【") ? { text: p.slice(1, -1), mark: true } : { text: p, mark: false }));
  }

  /** Takes a suggestion into the list: the aliases onto the entry that has the term, else a new entry at the top. */
  function accept(s: Suggestion) {
    if (!items) return;
    const existing = items.find((item) => item.term.trim().toLowerCase() === s.term.toLowerCase());
    if (existing) {
      for (const a of s.add_aliases) if (!existing.aliases.includes(a)) existing.aliases.push(a);
    } else {
      items.unshift({ term: s.term, aliases: [...s.add_aliases], asr_hint: true, key: nextKey++, pending: "" });
    }
    taken.push(s.term);
  }
  function dismiss(s: Suggestion) {
    turnedDown.push(s.term);
  }

  // Alias text typed but not yet turned into a chip counts as a change, so leaving the page asks first.
  const dirty = $derived(
    items !== null && stored !== null &&
      (JSON.stringify(toEntries(items)) !== JSON.stringify(stored) || items.some((item) => item.pending.trim() !== "") || turnedDown.length > 0),
  );

  /** An entry with aliases but no term blocks the save. */
  const blocks = (item: Item) => !item.term.trim() && (item.aliases.some((a) => a.trim()) || item.pending.trim() !== "");

  function removeLabel(item: Item): string {
    const first = item.term.split("\n").map((l) => l.trim()).find(Boolean);
    return first ? t("dictionary.remove_term", { term: first }) : t("dictionary.remove_blank");
  }

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });

  $effect(() => {
    // A new edit clears the previous validation message.
    if (items) JSON.stringify(toEntries(items));
    problem = undefined;
  });

  async function add() {
    if (!items) return;
    items.unshift({ term: "", aliases: [], asr_hint: true, key: nextKey++, pending: "" });
    await tick();
    list?.querySelector<HTMLTextAreaElement>("textarea")?.focus();
  }

  function commitAlias(item: Item) {
    if (!item.pending.trim()) { item.pending = ""; return; }
    item.aliases = addAliases(item.aliases, item.pending);
    item.pending = "";
  }

  function aliasKey(e: KeyboardEvent, item: Item) {
    if (isImeKey(e)) return;
    if (e.key === "Enter" || e.key === "," || e.key === "，") {
      e.preventDefault();
      commitAlias(item);
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
    return { update: fit, destroy: () => el.removeEventListener("input", fit) };
  }

  function revert() {
    if (!stored) return;
    items = toItems($state.snapshot(stored) as DictionaryEntry[]);
    synced = JSON.stringify(stored);
    taken = [];
    turnedDown = [];
  }

  async function save() {
    if (!items) return;
    for (const item of items) commitAlias(item);
    await tick();
    const sent = toEntries($state.snapshot(items) as Item[]);
    const entries = dropBlank(trimEntries(sent));
    if (entries.some((e) => e.term === "")) { problem = t("dictionary.term_required"); return; }
    saving = true;
    try {
      const dismissed = [...(config.value?.dictionary_dismissed ?? []), ...turnedDown.filter((d) => !(config.value?.dictionary_dismissed ?? []).includes(d))];
      const next = await setDictionary(entries, dismissed);
      config.value = next;
      taken = [];
      turnedDown = [];
      void loadSuggestions();
      // Edits made while the save was running stay in the editor.
      if (items && JSON.stringify(toEntries(items)) === JSON.stringify(sent) && !items.some((item) => item.pending.trim())) {
        items = toItems(structuredClone(next.dictionary));
      }
      synced = JSON.stringify(next.dictionary);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }
</script>

{#if items !== null}
  <PageHead title={t("nav.dictionary")} description={t("dictionary.note")}>
    {#snippet actions()}
      <button type="button" class="btn primary" onclick={add}><Icon name="plus" size={14} />{t("dictionary.add")}</button>
    {/snippet}
  </PageHead>

  {#if visible.length}
    <section class="sg" aria-label={t("dictionary.suggestions")}>
      <div class="sg-head" class:open={suggestionsOpen}>
        <button type="button" class="toggle" aria-expanded={suggestionsOpen} onclick={toggleSuggestions}>
          <span class="chev"><Icon name="chevron-right" size={14} /></span>
          <span class="title">{t("dictionary.suggestions")}</span>
          <small>{t("dictionary.suggestions_count", { n: String(visible.length) })}</small>
        </button>
        <InfoTip info={t("dictionary.suggestions_info")} />
      </div>
      {#if suggestionsOpen}
        <Fold>
          {#each shown as s (s.term)}
            <div class="sr">
              <div class="body">
                <div class="line">
                  <b>{s.term}</b>
                  <span class="heard">{t("dictionary.heard_as")}{#each s.aliases as a (a.text)}<span class="alias">{a.text}</span>{/each}</span>
                  <Tag>{t("dictionary.rows", { n: String(s.rows) })}</Tag>
                  {#if s.known_term}<Tag tone="accent">{t("dictionary.new_alias")}</Tag>{:else if !s.add_aliases.length}<Tag tone="warn">{t("dictionary.hint_only")}</Tag>{/if}
                </div>
                {#if s.excerpts[0]}<div class="excerpt">{#each pieces(s.excerpts[0]) as p, i (i)}{#if p.mark}<mark>{p.text}</mark>{:else}{p.text}{/if}{/each}</div>{/if}
              </div>
              <div class="acts">
                <button type="button" class="btn primary small" onclick={() => accept(s)}>{t("dictionary.accept")}</button>
                <button type="button" class="btn quiet small" onclick={() => dismiss(s)}>{t("dictionary.dismiss")}</button>
              </div>
            </div>
          {/each}
          {#if visible.length > shown.length}
            <button type="button" class="more" onclick={() => (showAll = true)}>{t("dictionary.more_suggestions", { n: String(visible.length - shown.length) })}</button>
          {/if}
        </Fold>
      {/if}
    </section>
    <p class="help">{t("dictionary.suggestions_help")}</p>
  {/if}

  {#if items.length}
    <div bind:this={list}>
      <Group help={t("dictionary.snippet_help")} info={t("dictionary.snippet_info")}>
        {#each items as item, i (item.key)}
          <Row>
            <div class="entry">
              <div class="line">
                <textarea
                  class="term"
                  rows="1"
                  bind:value={item.term}
                  use:grow={item.term}
                  placeholder={t("dictionary.term")}
                  aria-label={t("dictionary.term")}
                  aria-invalid={problem && blocks(item) ? "true" : undefined}
                ></textarea>
                {#if isSnippet(item.term)}<span class="tag"><Tag tone="accent">{t("dictionary.snippet")}</Tag></span>{/if}
                <button type="button" class="btn quiet icon-btn" aria-label={removeLabel(item)} use:tip={t("common.remove")} onclick={() => items!.splice(i, 1)}><Icon name="trash" /></button>
              </div>
              <div class="line second">
                <span class="sub lead">{t("dictionary.aliases")}</span>
                <div class="aliases">
                  {#each item.aliases as a, j (j)}
                    <Chip onremove={() => item.aliases.splice(j, 1)}>{a}</Chip>
                  {/each}
                  <input
                    type="text"
                    class="alias-input"
                    bind:value={item.pending}
                    placeholder={t("dictionary.aliases_placeholder")}
                    aria-label={t("dictionary.aliases")}
                    onkeydown={(e) => aliasKey(e, item)}
                    onblur={() => commitAlias(item)}
                  />
                </div>
                <span class="hint">
                  <span class="sub">{t("dictionary.asr_hint")}</span>
                  <Switch bind:checked={item.asr_hint} label={t("dictionary.asr_hint")} />
                </span>
              </div>
            </div>
          </Row>
        {/each}
      </Group>
    </div>
  {:else}
    <Group>
      <Empty text={t("dictionary.empty")}>
        {#snippet action()}<button type="button" class="btn" onclick={add}><Icon name="plus" size={14} />{t("dictionary.add")}</button>{/snippet}
      </Empty>
    </Group>
  {/if}

  <SaveBar {dirty} {saving} message={problem} onsave={save} onrevert={revert} />
{/if}

<style>
  .sg { margin-top: 22px; border: 1px solid var(--line); border-radius: var(--radius-group); background: var(--surface); }
  .sg-head { display: flex; align-items: center; gap: 4px; padding-right: 12px; }
  .toggle { display: flex; align-items: center; gap: 10px; flex: 1; min-width: 0; min-height: 44px; padding: 0 16px; border: 0; border-radius: var(--radius-group); background: none; color: var(--ink); text-align: left; font-size: var(--text-control); cursor: default; }
  .toggle:hover { background: var(--sunken); }
  .open .toggle { border-radius: var(--radius-group) var(--radius-group) 0 0; }
  .toggle:focus-visible { outline-offset: -2px; }
  .chev { display: inline-grid; place-items: center; width: 14px; color: var(--ink-3); transition: transform .2s; }
  .open .chev { transform: rotate(90deg); }
  .toggle .title { font-weight: 600; }
  .toggle small { color: var(--ink-2); font-size: var(--text-sm); }
  .sr { display: flex; align-items: flex-start; gap: 12px; padding: 11px 16px; border-top: 1px solid var(--line); }
  .sr .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .sr .line { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: var(--text-control); }
  .sr b { font-weight: 600; }
  .heard { display: inline-flex; align-items: center; gap: 6px; color: var(--ink-2); }
  .alias { color: var(--ink-2); }
  .excerpt { color: var(--ink-2); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .excerpt mark { background: var(--accent-soft); color: var(--ink); border-radius: 4px; padding: 0 2px; }
  .acts { flex: none; display: flex; gap: 4px; margin-top: 1px; }
  .btn.small { height: 28px; padding: 0 10px; font-size: var(--text-sm); }
  .more { display: block; width: 100%; padding: 10px 16px; border: 0; border-top: 1px solid var(--line); background: none; color: var(--accent); text-align: left; font-size: var(--text-control); cursor: default; }
  .more:hover { background: var(--sunken); }
  .entry { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; padding: 2px 0; }
  .line { display: flex; align-items: flex-start; gap: 10px; min-width: 0; }
  .second { align-items: center; }
  /* One line is as tall as the other single-line fields. */
  .term { flex: 1; min-width: 0; min-height: 0; height: var(--control); padding: 5px 10px; border-radius: var(--radius); line-height: 20px; font-size: var(--text-control); resize: none; overflow: hidden; }
  .tag { flex: none; margin-top: 5px; }
  .icon-btn { width: var(--control); padding: 0; margin: 0 -8px 0 0; color: var(--ink-2); flex: none; }
  .icon-btn:hover { color: var(--ink); }
  .lead { flex: none; }
  .aliases { flex: 1; min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .alias-input { flex: 1; min-width: 90px; height: 28px; padding: 0 4px; border: 0; background: none; font-size: var(--text-sm); }
  .hint { flex: none; display: flex; align-items: center; gap: 8px; }
</style>
