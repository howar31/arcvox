<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { dictionaryExamples, dictionaryHits, dictionaryIndex, dictionarySuggestions, errorMessage, setDictionary, type DictionaryEntry, type Dismissed, type Suggestion, type TermHits, type TermIndex } from "../../lib/api";
  import { dropBlank, isSnippet, trimEntries } from "../../lib/dictionary";
  import { arrange, matches, rememberSort, rememberView, SORTS, storedSort, storedView, VIEWS, type Labels, type Sort, type View } from "../../lib/dictionaryView";
  import { guard } from "../../lib/guard.svelte";
  import { isImeKey } from "../../lib/ime";
  import { formatShortDate } from "../../lib/format";
  import { t } from "../../lib/i18n";
  import { config } from "../../lib/store.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Fold from "../../lib/ui/Fold.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import InfoTip from "../../lib/ui/InfoTip.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { tip } from "../../lib/tip";
  import { toast } from "../../lib/ui/toast.svelte";
  import DictionaryEditor from "./DictionaryEditor.svelte";

  /** An entry being edited: `key` keeps each row's elements stable, `pending` is the alias being typed. */
  type Item = DictionaryEntry & { key: number; pending: string };
  type Tab = "terms" | "suggestions" | "ignored";
  /** The card grid's columns; the editor opens under the chosen card's row. Kept with the `.grid` style. */
  const CARD_COLUMNS = 3;

  let nextKey = 0;
  const toItems = (entries: DictionaryEntry[]): Item[] =>
    entries.map((e) => ({ ...structuredClone(e), key: nextKey++, pending: "" }));
  const toEntries = (items: Item[]): DictionaryEntry[] =>
    items.map(({ key: _k, pending: _p, ...e }) => ({ ...e, aliases: [...e.aliases] }));
  const now = () => Math.floor(Date.now() / 1000);

  const stored = $derived(config.value?.dictionary ?? null);
  const storedDismissed = $derived(config.value?.dictionary_dismissed ?? []);
  let items = $state<Item[] | null>(null);
  // The stored dictionary the items were last copied from, and each item's stored form, for `updated`.
  let synced = "";
  let original = new Map<number, string>();
  let saving = $state(false);
  let problem = $state<string | undefined>(undefined);

  $effect(() => {
    if (!stored) return;
    const next = JSON.stringify(stored);
    untrack(() => {
      if (items === null || (JSON.stringify(toEntries(items)) === synced && !items.some((item) => item.pending.trim()))) {
        items = toItems($state.snapshot(stored) as DictionaryEntry[]);
        synced = next;
        original = new Map(items.map((i) => [i.key, JSON.stringify(toEntries([i])[0])]));
      }
    });
  });

  // The page's own state: the tab, the view, the sort and the search; view and sort are remembered.
  let tab = $state<Tab>("terms");
  let view = $state<View>(storedView());
  let sort = $state<Sort>(storedSort());
  let query = $state("");
  let selected = $state<number | null>(null);
  let fresh = $state<number | null>(null);
  function setView(v: View) { view = v; rememberView(v); }
  function setSort(v: Sort) { sort = v; rememberSort(v); }

  // Sort keys of every term, from the core; refreshed when the set of terms changes.
  let index = $state<Map<string, TermIndex>>(new Map());
  $effect(() => {
    const terms = [...new Set((items ?? []).map((i) => i.term.trim()).filter(Boolean))];
    const missing = terms.filter((x) => !untrack(() => index).has(x));
    if (!missing.length) return;
    dictionaryIndex(missing).then((keys) => {
      const next = new Map(untrack(() => index));
      missing.forEach((x, i) => next.set(x, keys[i]));
      index = next;
    }).catch(() => {});
  });

  // How often each term's aliases were replaced, from the records.
  let hits = $state<Map<string, TermHits>>(new Map());
  const hitsOf = (item: Item): TermHits | null => hits.get(item.term.trim()) ?? null;
  const aliasText = (n: number): string => (n === 0 ? t("dictionary.no_aliases") : n === 1 ? t("dictionary.alias_one") : t("dictionary.alias_count", { n: String(n) }));
  const hitsText = (item: Item): string => { const h = hitsOf(item); return !h ? "" : h.count === 1 ? t("dictionary.hits_one") : t("dictionary.hits_n", { n: String(h.count) }); };

  const labels = $derived<Labels>({
    strokes: (n) => t("dictionary.strokes_n", { n: String(n) }),
    latin: "", kana: t("dictionary.sec_kana"), other: t("dictionary.sec_other"), unknown: t("dictionary.sec_unknown"),
    manual: t("dictionary.origin_manual"), suggestion: t("dictionary.origin_suggestion"), snippet: t("dictionary.snippet"), term: t("dictionary.tab_terms"),
  });
  const shownItems = $derived((items ?? []).filter((i) => matches(i, query)));
  const sections = $derived(arrange(shownItems, sort, shownItems.map((i) => index.get(i.term.trim()) ?? null), labels, hits));
  const sectioned = $derived(sections.length > 1 || (sections.length === 1 && sections[0].label !== ""));
  const selectedItem = $derived(items?.find((i) => i.key === selected) ?? null);

  // Sentences the chosen term appears in, from the history.
  let examples = $state<{ term: string; list: string[] } | null>(null);
  $effect(() => {
    const term = selectedItem?.term.trim() ?? "";
    if (!term) { examples = null; return; }
    if (untrack(() => examples)?.term === term) return;
    const timer = setTimeout(() => {
      dictionaryExamples(term, 2).then((list) => { examples = { term, list }; }).catch(() => { examples = { term, list: [] }; });
    }, 300);
    return () => clearTimeout(timer);
  });
  const examplesFor = (item: Item): string[] | null => (examples?.term === item.term.trim() ? examples.list : null);

  // Suggestions: terms the dictations the user corrected were heard as something else.
  let suggestions = $state<Suggestion[] | null>(null);
  let showAll = $state(false);
  // Acted on in this draft: taken into the list, turned down, or brought back from the ignored list.
  let taken = $state<string[]>([]);
  let turnedDown = $state<Dismissed[]>([]);
  let restored = $state<string[]>([]);
  // The core lists every candidate; the page leaves out the ignored ones, so a restored term shows at once.
  // Newest first; entries ignored before the time was kept come after, in their stored order.
  const ignored = $derived(
    [...turnedDown, ...storedDismissed.filter((d) => !turnedDown.some((x) => x.term === d.term))]
      .filter((d) => !restored.includes(d.term))
      .map((d, i) => ({ d, i }))
      .sort((a, b) => (b.d.at ?? 0) - (a.d.at ?? 0) || a.i - b.i)
      .map((x) => x.d),
  );
  const hit = (texts: string[]) => { const q = query.replace(/\s+/g, "").toLowerCase(); return !q || texts.some((x) => x.replace(/\s+/g, "").toLowerCase().includes(q)); };
  const visible = $derived((suggestions ?? []).filter((s) => !taken.includes(s.term) && !ignored.some((d) => d.term === s.term) && hit([s.term, ...s.aliases.map((a) => a.text)])));
  const shown = $derived(showAll ? visible : visible.slice(0, 6));
  let restoreSure = $state(false);
  let restoreTimer: ReturnType<typeof setTimeout> | undefined;

  async function loadSuggestions() {
    try { suggestions = await dictionarySuggestions(); } catch (e) { suggestions = []; toast.error(errorMessage(e)); }
  }
  onMount(() => {
    void loadSuggestions();
    dictionaryHits().then((list) => { hits = new Map(list.map((h) => [h.term, h])); }).catch(() => {});
  });

  /** The excerpt's pieces: the term is between 【 and 】. */
  function pieces(excerpt: string): { text: string; mark: boolean }[] {
    return excerpt.split(/(【[^】]*】)/).filter(Boolean).map((p) => (p.startsWith("【") ? { text: p.slice(1, -1), mark: true } : { text: p, mark: false }));
  }
  const form = (s: string) => s.replace(/\s+/g, "").toLowerCase();

  /** Takes a suggestion into the list: onto the entry that has the term (in any casing or spacing) or the
      entry it is a letter or two away from, as aliases; else a new entry, which the Terms tab then shows open. */
  function accept(s: Suggestion) {
    if (!items) return;
    const existing = items.find((item) => form(item.term) === form(s.term)) ?? (s.near_term ? items.find((item) => item.term.trim() === s.near_term) : undefined);
    const heard = s.aliases.map((a) => a.text);
    if (existing) {
      const aliases = existing.term.trim() === s.term ? s.add_aliases : [s.term, ...s.add_aliases];
      for (const a of aliases) if (!existing.aliases.includes(a) && a !== existing.term.trim()) existing.aliases.push(a);
      existing.heard = [...new Set([...(existing.heard ?? []), ...heard])];
    } else {
      items.unshift({ term: s.term, aliases: [...s.add_aliases], asr_hint: true, origin: "suggestion", heard, added: now(), key: nextKey++, pending: "" });
    }
    taken.push(s.term);
  }
  function dismiss(s: Suggestion) {
    turnedDown.push({ term: s.term, heard: s.aliases.map((a) => a.text), at: now() });
  }
  function restore(d: Dismissed) {
    if (turnedDown.some((x) => x.term === d.term)) turnedDown = turnedDown.filter((x) => x.term !== d.term);
    else restored.push(d.term);
  }
  // Ignored terms the current rules would not suggest any more: left over from earlier rules, or their corrections expired.
  const stale = $derived(suggestions ? ignored.filter((d) => !suggestions!.some((x) => x.term === d.term)) : []);
  let staleSure = $state(false);
  let staleTimer: ReturnType<typeof setTimeout> | undefined;
  function dropStale() {
    if (!staleSure) {
      staleSure = true;
      clearTimeout(staleTimer);
      staleTimer = setTimeout(() => { staleSure = false; }, 4000);
      return;
    }
    staleSure = false;
    for (const d of stale) restore(d);
  }
  function restoreAll() {
    if (!restoreSure) {
      restoreSure = true;
      clearTimeout(restoreTimer);
      restoreTimer = setTimeout(() => { restoreSure = false; }, 4000);
      return;
    }
    restoreSure = false;
    for (const d of ignored) restore(d);
  }

  const dirty = $derived(
    items !== null && stored !== null &&
      (JSON.stringify(toEntries(items)) !== JSON.stringify(stored) || items.some((item) => item.pending.trim() !== "") || turnedDown.length > 0 || restored.length > 0),
  );
  /** An entry with aliases but no term blocks the save. */
  const blocks = (item: Item) => !item.term.trim() && (item.aliases.some((a) => a.trim()) || item.pending.trim() !== "");

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });
  $effect(() => {
    if (items) JSON.stringify(toEntries(items));
    problem = undefined;
  });

  async function add() {
    if (!items) return;
    const item: Item = { term: "", aliases: [], asr_hint: true, origin: "manual", added: now(), key: nextKey++, pending: "" };
    items.unshift(item);
    tab = "terms";
    query = "";
    selected = item.key;
    fresh = item.key;
    await tick();
  }
  function choose(item: Item) {
    selected = selected === item.key ? null : item.key;
    fresh = null;
  }
  function remove(item: Item) {
    if (!items) return;
    items.splice(items.findIndex((i) => i.key === item.key), 1);
    if (selected === item.key) selected = null;
  }

  function revert() {
    if (!stored) return;
    items = toItems($state.snapshot(stored) as DictionaryEntry[]);
    synced = JSON.stringify(stored);
    original = new Map(items.map((i) => [i.key, JSON.stringify(toEntries([i])[0])]));
    taken = [];
    turnedDown = [];
    restored = [];
    selected = null;
  }

  async function save() {
    if (!items) return;
    const stamp = now();
    for (const item of items) {
      if (item.pending.trim()) { item.aliases = [...item.aliases, ...item.pending.split(/[,，\s]+/).map((a) => a.trim()).filter((a) => a && !item.aliases.includes(a))]; }
      item.pending = "";
      const before = original.get(item.key);
      if (before !== undefined && before !== JSON.stringify(toEntries([item])[0])) item.updated = stamp;
    }
    await tick();
    const sent = toEntries($state.snapshot(items) as Item[]);
    const entries = dropBlank(trimEntries(sent));
    if (entries.some((e) => e.term === "")) { problem = t("dictionary.term_required"); return; }
    saving = true;
    try {
      const dismissed = [...storedDismissed.filter((d) => !restored.includes(d.term) && !turnedDown.some((x) => x.term === d.term)), ...turnedDown];
      const next = await setDictionary(entries, dismissed);
      config.value = next;
      taken = [];
      turnedDown = [];
      restored = [];
      void loadSuggestions();
      if (items && JSON.stringify(toEntries(items)) === JSON.stringify(sent) && !items.some((item) => item.pending.trim())) {
        const keep = selectedItem?.term;
        items = toItems(structuredClone(next.dictionary));
        selected = keep ? (items.find((i) => i.term === keep)?.key ?? null) : null;
      }
      synced = JSON.stringify(next.dictionary);
      original = new Map(items!.map((i) => [i.key, JSON.stringify(toEntries([i])[0])]));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  const tabs = $derived([
    { value: "terms" as Tab, label: `${t("dictionary.tab_terms")} ${items?.length ?? 0}` },
    { value: "suggestions" as Tab, label: `${t("dictionary.tab_suggestions")} ${visible.length}` },
    { value: "ignored" as Tab, label: `${t("dictionary.tab_ignored")} ${ignored.length}` },
  ]);
  const firstLine = (term: string) => term.split("\n").map((l) => l.trim()).find(Boolean) ?? "";
  /** What the right side of a row or the corner of a card says about an entry. */
  const status = (item: Item): { text: string; tone?: "accent" | "neutral" } | null =>
    isSnippet(item.term) ? { text: t("dictionary.snippet"), tone: "accent" } : !item.asr_hint ? { text: t("dictionary.hint_off"), tone: "neutral" } : null;
  function jump(key: string) {
    document.getElementById(`dict-sec-${key}`)?.scrollIntoView({ block: "start", behavior: "smooth" });
  }
  /** The date a list row shows: when it was changed under that sort, else when it was added. */
  function when(item: Item): string {
    if (sort === "hits") { const h = hitsOf(item); return h ? t("dictionary.hit_on", { date: formatShortDate(h.last) }) : ""; }
    if (sort === "updated" && item.updated) return t("dictionary.updated_on", { date: formatShortDate(item.updated) });
    return item.added ? t("dictionary.added_on", { date: formatShortDate(item.added) }) : "";
  }
  // The page scrolls inside the settings window's main area; a button brings it back to the top once it is some way down.
  let scroller: HTMLElement | null = null;
  let scrolled = $state(false);
  function watch(el: HTMLElement) {
    scroller = el.closest("main");
    const onScroll = () => { scrolled = (scroller?.scrollTop ?? 0) > 360; };
    scroller?.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return { destroy: () => scroller?.removeEventListener("scroll", onScroll) };
  }
  function toTop() {
    scroller?.scrollTo({ top: 0, behavior: "smooth" });
  }
  function searchKey(e: KeyboardEvent) {
    if (isImeKey(e)) return;
    if (e.key === "Escape") query = "";
  }
</script>

{#if items !== null}
  <div class="watch" use:watch></div>
  <PageHead title={t("nav.dictionary")} description={t("dictionary.note")}>
    {#snippet actions()}
      <button type="button" class="btn primary" onclick={add}><Icon name="plus" size={14} />{t("dictionary.add")}</button>
    {/snippet}
  </PageHead>

  <div class="bar">
    <Segmented options={tabs} bind:value={tab} label={t("nav.dictionary")} />
    <label class="search"><Icon name="search" /><input type="search" bind:value={query} placeholder={t("dictionary.search")} aria-label={t("dictionary.search")} onkeydown={searchKey} /></label>
    {#if tab === "terms"}
      <select class="sort" value={sort} onchange={(e) => setSort(e.currentTarget.value as Sort)} aria-label={t("dictionary.sort")}>
        {#each SORTS as s (s)}<option value={s}>{t(`dictionary.sort_${s}`)}</option>{/each}
      </select>
      <span class="views" role="group" aria-label={t("dictionary.view_cards")}>
        {#each VIEWS as v (v)}
          <button type="button" class:on={view === v} aria-pressed={view === v} use:tip={t(`dictionary.view_${v}`)} onclick={() => setView(v)}><Icon name={`view-${v}`} size={16} /></button>
        {/each}
      </span>
    {:else if tab === "ignored" && ignored.length}
      {#if stale.length}
        <button type="button" class="btn quiet" class:danger={staleSure} onclick={dropStale}>{t(staleSure ? "dictionary.drop_stale_sure" : "dictionary.drop_stale", { n: String(stale.length) })}</button>
      {/if}
      <button type="button" class="btn quiet" class:danger={restoreSure} onclick={restoreAll}>{restoreSure ? t("dictionary.restore_all_sure", { n: String(ignored.length) }) : t("dictionary.restore_all")}</button>
    {/if}
  </div>

  {#if tab === "terms"}
    {#if !items.length}
      <Group>
        <Empty text={t("dictionary.empty")}>
          {#snippet action()}<button type="button" class="btn" onclick={add}><Icon name="plus" size={14} />{t("dictionary.add")}</button>{/snippet}
        </Empty>
      </Group>
    {:else if !shownItems.length}
      <Group><Empty text={t("dictionary.no_match")} /></Group>
    {:else}
      <div class="body" class:railed={sectioned && view !== "panes"}>
        <div class="content">

        {#if view === "cards"}
          <div class="grid">
            {#each sections as s (s.key)}
              {@const at = s.items.findIndex((i) => i.key === selected)}
              {@const after = at < 0 ? -1 : Math.min(Math.floor(at / CARD_COLUMNS) * CARD_COLUMNS + CARD_COLUMNS - 1, s.items.length - 1)}
              {#if s.label}<div class="gsec" id="dict-sec-{s.key}">{s.label}</div>{/if}
              {#each s.items as item, i (item.key)}
                <button type="button" class="card" class:sel={selected === item.key} aria-expanded={selected === item.key} onclick={() => choose(item)}>
                  <span class="ct"><span class="name" class:blank={!item.term.trim()}>{firstLine(item.term) || t("dictionary.term")}</span>
                    {#if status(item)}<span class="corner"><Tag tone={status(item)!.tone}>{status(item)!.text}</Tag></span>{:else if hitsOf(item)}<span class="corner num" use:tip={t("dictionary.hits_note")}>{hitsText(item)}</span>{:else if item.aliases.length}<span class="corner num">{aliasText(item.aliases.length)}</span>{/if}
                  </span>
                  {#if item.aliases.length}
                    <span class="als">{#each item.aliases.slice(0, 3) as a, i (i)}<span class="chip">{a}</span>{/each}{#if item.aliases.length > 3}<span class="chip more">+{item.aliases.length - 3}</span>{/if}</span>
                  {:else}<span class="none">{t("dictionary.no_aliases")}</span>{/if}
                </button>
                <!-- The editor goes under the chosen card's whole row, so the cards beside it stay where they are. -->
                {#if i === after && selectedItem}
                  <div class="ged"><Fold><div class="edbox"><DictionaryEditor bind:item={items[items.findIndex((x) => x.key === selectedItem!.key)]} examples={examplesFor(selectedItem)} hits={hitsOf(selectedItem)} invalid={!!problem && blocks(selectedItem)} autofocus={fresh === selectedItem.key} onremove={() => remove(selectedItem!)} /></div></Fold></div>
                {/if}
              {/each}
            {/each}
          </div>
        {:else if view === "list"}
          <Group>
            {#each sections as s (s.key)}
              {#if s.label}<div class="lsec" id="dict-sec-{s.key}">{s.label}</div>{/if}
              {#each s.items as item (item.key)}
                <button type="button" class="row" class:sel={selected === item.key} aria-expanded={selected === item.key} onclick={() => choose(item)}>
                  <span class="name" class:blank={!item.term.trim()}>{firstLine(item.term) || t("dictionary.term")}</span>
                  <span class="als">{#each item.aliases as a, i (i)}<span class="chip">{a}</span>{/each}</span>
                  <span class="right">{#if when(item)}<span class="num">{when(item)}</span>{/if}{#if hitsOf(item)}<span class="num" use:tip={t("dictionary.hits_note")}>{hitsText(item)}</span>{/if}{#if status(item)}<Tag tone={status(item)!.tone}>{status(item)!.text}</Tag>{:else}<span class="num">{aliasText(item.aliases.length)}</span>{/if}</span>
                </button>
                {#if selected === item.key}
                  <Fold><div class="edrow"><DictionaryEditor bind:item={items[items.findIndex((i) => i.key === item.key)]} examples={examplesFor(item)} hits={hitsOf(item)} invalid={!!problem && blocks(item)} autofocus={fresh === item.key} onremove={() => remove(item)} /></div></Fold>
                {/if}
              {/each}
            {/each}
          </Group>
        {:else}
          <div class="panes">
            <Group>
              {#each sections as s (s.key)}
                {#if s.label}<div class="lsec" id="dict-sec-{s.key}">{s.label}</div>{/if}
                {#each s.items as item (item.key)}
                  <button type="button" class="prow" class:sel={selected === item.key} aria-pressed={selected === item.key} onclick={() => { selected = item.key; fresh = null; }}>
                    <span class="name" class:blank={!item.term.trim()}>{firstLine(item.term) || t("dictionary.term")}</span>
                    <span class="meta">{#if status(item)}{status(item)!.text}{:else if hitsOf(item)}<span use:tip={t("dictionary.hits_note")}>{hitsText(item)}</span>{:else if item.aliases.length}{aliasText(item.aliases.length)}{/if}</span>
                  </button>
                {/each}
              {/each}
            </Group>
            <div class="pane">
              <Group>
                {#if selectedItem}
                  <div class="edrow pane-ed"><DictionaryEditor bind:item={items[items.findIndex((i) => i.key === selectedItem!.key)]} examples={examplesFor(selectedItem)} hits={hitsOf(selectedItem)} invalid={!!problem && blocks(selectedItem)} autofocus={fresh === selectedItem.key} onremove={() => remove(selectedItem!)} /></div>
                {:else}
                  <Empty text={t("dictionary.select_hint")} />
                {/if}
              </Group>
            </div>
          </div>
        {/if}
        </div>
        {#if sectioned && view !== "panes"}
          <nav class="rail" aria-label={t("dictionary.sort")}>
            <button type="button" class="top" use:tip={t("dictionary.to_top")} onclick={toTop}><Icon name="chevron-up" size={12} /></button>
            {#each sections as s (s.key)}<button type="button" onclick={() => jump(s.key)}>{s.label}</button>{/each}
          </nav>
        {/if}
      </div>
    {/if}
    <p class="help">{t("dictionary.snippet_help")}<InfoTip info={t("dictionary.snippet_info")} /></p>
    {#if sort === "strokes" || sort === "zhuyin" || sort === "pinyin"}<p class="help">{t("dictionary.sort_note")}</p>{/if}
    {#if sort === "hits"}<p class="help">{t("dictionary.hits_note")}</p>{/if}
  {:else if tab === "suggestions"}
    {#if !visible.length}
      <Group><Empty text={t("dictionary.suggestions_empty")} /></Group>
    {:else}
      <Group>
        {#each shown as s (s.term)}
          <div class="sr">
            <div class="sbody">
              <div class="line">
                <b>{s.term}</b>
                <span class="heard">{t("dictionary.heard_as")}{#each s.aliases as a (a.text)}<span class="alias">{a.text}</span>{/each}</span>
                <Tag>{t("dictionary.rows", { n: String(s.rows) })}</Tag>
                {#if s.known_term}<Tag tone="accent">{t("dictionary.new_alias")}</Tag>{:else if s.near_term}<Tag tone="accent">{t("dictionary.near_term", { term: s.near_term })}</Tag>{:else if !s.add_aliases.length}<Tag tone="warn">{t(s.sound_alike ? "dictionary.sound_alike" : "dictionary.hint_only")}</Tag>{/if}
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
          <button type="button" class="more" onclick={() => (showAll = true)}>{t("dictionary.more", { n: String(visible.length - shown.length) })}</button>
        {/if}
      </Group>
    {/if}
    <p class="help">{t("dictionary.suggestions_help")}<InfoTip info={t("dictionary.suggestions_info")} /></p>
  {:else}
    {#if !ignored.length}
      <Group><Empty text={t("dictionary.ignored_empty")} /></Group>
    {:else}
      <Group>
        {#each ignored.filter((d) => hit([d.term, ...(d.heard ?? [])])) as d (d.term)}
          {@const live = suggestions?.find((x) => x.term === d.term)}
          {@const heard = d.heard?.length ? d.heard : (live?.aliases.map((a) => a.text) ?? [])}
          <div class="irow">
            <span class="name">{d.term}</span>
            {#if heard.length}<span class="meta">{t("dictionary.heard_as")} {heard.join(t("common.list_sep"))}</span>{/if}
            {#if suggestions && !live}<Tag>{t("dictionary.not_suggested_now")}</Tag>{/if}
            <span class="grow"></span>
            <button type="button" class="btn quiet small" onclick={() => restore(d)}>{t(suggestions && !live ? "common.remove" : "dictionary.restore")}</button>
          </div>
        {/each}
      </Group>
    {/if}
    <p class="help">{t("dictionary.ignored_help")}</p>
  {/if}

  {#if scrolled && !dirty}
    <button type="button" class="totop" use:tip={t("dictionary.to_top")} aria-label={t("dictionary.to_top")} onclick={toTop}><Icon name="chevron-up" size={16} /></button>
  {/if}
  <SaveBar {dirty} {saving} message={problem} onsave={save} onrevert={revert} />
{/if}

<style>
  .bar { display: flex; align-items: center; gap: 8px; margin: 0 0 12px; }
  .search { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; height: var(--control); padding: 0 10px; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--surface); color: var(--ink-3); }
  .search input { flex: 1; min-width: 0; height: 100%; border: 0; background: none; padding: 0; font-size: var(--text-control); }
  .sort { height: var(--control); flex: none; }
  .views { display: inline-flex; padding: 3px; gap: 2px; border-radius: var(--radius); background: var(--sunken); flex: none; }
  .views button { display: inline-grid; place-items: center; width: 30px; height: 26px; border: 0; border-radius: 7px; background: none; color: var(--ink-2); cursor: default; }
  .views button.on { background: var(--surface); color: var(--ink); box-shadow: 0 1px 2px rgba(0, 0, 0, .08); }
  .grow { flex: 1; }
  .help { margin: 6px 4px 0; color: var(--ink-2); font-size: var(--text-sm); }
  .body { display: flex; align-items: flex-start; }
  .content { flex: 1; min-width: 0; }
  /* The rail sits in the page's right margin and stays in view while the list scrolls. */
  .rail { position: sticky; top: 8px; flex: none; display: flex; flex-direction: column; align-items: center; width: 40px; margin-right: -44px; margin-left: 4px; }
  .rail button { border: 0; background: none; padding: 1px 0; color: var(--ink-3); font-size: 11px; font-weight: 600; line-height: 1.3; white-space: nowrap; cursor: default; }
  .rail button:hover { color: var(--accent); }
  .rail .top { display: inline-grid; place-items: center; width: 22px; height: 22px; margin-bottom: 4px; border-radius: 50%; background: var(--sunken); color: var(--ink-2); }
  .totop { position: fixed; right: 28px; bottom: 28px; z-index: 5; display: inline-grid; place-items: center; width: 36px; height: 36px; border: 1px solid var(--line-strong); border-radius: 50%; background: var(--surface); color: var(--ink-2); box-shadow: 0 4px 14px rgba(0, 0, 0, .12); cursor: default; }
  .totop:hover { color: var(--accent); border-color: var(--accent); }
  .watch { display: none; }
  .name { font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name.blank { color: var(--ink-3); font-weight: 400; }
  .chip { display: inline-flex; align-items: center; height: 22px; padding: 0 7px; border-radius: 7px; background: var(--sunken); color: var(--ink); font-size: var(--text-xs); white-space: nowrap; }
  .chip.more { color: var(--ink-2); }
  .num { color: var(--ink-3); font-size: var(--text-xs); font-variant-numeric: tabular-nums; }
  /* cards */
  .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
  .gsec { grid-column: 1 / -1; padding: 8px var(--row-x) 0; font-size: var(--text-xs); font-weight: 700; color: var(--ink-3); }
  /* The same side padding as a list row, so the text starts at one edge in every view. */
  .card { display: flex; flex-direction: column; gap: 6px; min-height: 74px; padding: 10px var(--row-x); border: 1px solid var(--line); border-radius: 12px; background: var(--surface); text-align: left; color: var(--ink); cursor: default; min-width: 0; }
  .card:hover { background: color-mix(in srgb, var(--sunken) 55%, var(--surface)); }
  .card.sel { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }
  .ct { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .ct .name { font-size: var(--text-control); }
  .corner { margin-left: auto; flex: none; }
  .card .als { display: flex; gap: 4px; flex-wrap: wrap; overflow: hidden; max-height: 48px; }
  .none { color: var(--ink-3); font-size: var(--text-xs); }
  .ged { grid-column: 1 / -1; }
  .edbox { padding: 12px 16px 14px; border: 1px solid var(--accent); border-radius: 12px; background: var(--surface); }
  /* list */
  .lsec { padding: 8px 16px 4px; font-size: var(--text-xs); font-weight: 700; color: var(--ink-3); background: color-mix(in srgb, var(--sunken) 55%, transparent); border-top: 1px solid var(--line); }
  .lsec:first-child { border-top: 0; }
  /* A fixed name column lines the aliases up; the date and the count sit in columns of their own on the right. */
  .row { display: flex; align-items: center; gap: 16px; width: 100%; min-height: 50px; padding: 10px var(--row-x); border: 0; border-top: 1px solid var(--line); background: none; color: var(--ink); text-align: left; cursor: default; }
  .row .name { flex: 0 0 200px; font-size: var(--text-control); }
  .row .chip { height: 24px; padding: 0 9px; font-size: var(--text-sm); }
  .row .right .num { display: inline-block; min-width: 76px; text-align: right; font-size: var(--text-sm); }
  .row .right .num:first-child:not(:last-child) { min-width: 92px; }
  .row:first-child, .lsec + .row { border-top: 0; }
  .row:hover { background: color-mix(in srgb, var(--sunken) 55%, transparent); }
  .row.sel { background: var(--accent-soft); }
  .row .als { display: flex; gap: 6px; flex-wrap: wrap; min-width: 0; }
  .row .right { margin-left: auto; flex: none; display: inline-flex; align-items: center; gap: 14px; }
  .edrow { padding: 12px var(--row-x) 14px; border-top: 1px solid var(--line); background: color-mix(in srgb, var(--sunken) 55%, transparent); }
  /* panes */
  .panes { display: grid; grid-template-columns: 268px minmax(0, 1fr); gap: 8px; align-items: start; }
  .prow { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 40px; padding: 8px var(--row-x); border: 0; border-top: 1px solid var(--line); background: none; color: var(--ink); text-align: left; cursor: default; }
  .prow:first-child, .lsec + .prow { border-top: 0; }
  .prow:hover { background: color-mix(in srgb, var(--sunken) 55%, transparent); }
  .prow.sel { background: var(--accent-soft); }
  .prow .name { font-weight: 500; font-size: var(--text-control); flex: 1; }
  .meta { color: var(--ink-3); font-size: var(--text-xs); white-space: nowrap; }
  .pane { position: sticky; top: 8px; }
  .pane-ed { border-top: 0; background: none; }
  /* suggestions */
  .sr { display: flex; align-items: flex-start; gap: 12px; padding: 11px var(--row-x); border-top: 1px solid var(--line); }
  .sr:first-child { border-top: 0; }
  .sbody { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .sr .line { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: var(--text-control); }
  .sr b { font-weight: 600; }
  .heard { display: inline-flex; align-items: center; gap: 6px; color: var(--ink-2); }
  .alias { color: var(--ink-2); }
  .excerpt { color: var(--ink-2); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .excerpt mark { background: var(--accent-soft); color: var(--ink); border-radius: 4px; padding: 0 2px; }
  .acts { flex: none; display: flex; gap: 4px; margin-top: 1px; }
  .btn.small { height: 28px; padding: 0 10px; font-size: var(--text-sm); }
  .more { display: block; width: 100%; padding: 10px var(--row-x); border: 0; border-top: 1px solid var(--line); background: none; color: var(--accent); text-align: left; font-size: var(--text-control); cursor: default; }
  .more:hover { background: var(--sunken); }
  /* ignored */
  .irow { display: flex; align-items: center; gap: 10px; min-height: 42px; padding: 7px var(--row-x); border-top: 1px solid var(--line); }
  .irow:first-child { border-top: 0; }
  .irow .name { font-weight: 500; }
  .irow .btn { margin-right: -8px; }
</style>
