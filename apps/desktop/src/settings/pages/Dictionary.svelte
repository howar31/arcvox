<script lang="ts">
  import { tick, untrack } from "svelte";
  import { errorMessage, setDictionary, type DictionaryEntry } from "../../lib/api";
  import { addAliases, dropBlank, isSnippet, trimEntries } from "../../lib/dictionary";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import { config } from "../../lib/store.svelte";
  import Chip from "../../lib/ui/Chip.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
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
      if (items === null || JSON.stringify(toEntries(items)) === synced) {
        items = toItems($state.snapshot(stored) as DictionaryEntry[]);
        synced = next;
      }
    });
  });

  const dirty = $derived(items !== null && stored !== null && JSON.stringify(toEntries(items)) !== JSON.stringify(stored));

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
    if (e.isComposing) return;
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
  }

  async function save() {
    if (!items) return;
    for (const item of items) commitAlias(item);
    await tick();
    const entries = dropBlank(trimEntries(toEntries($state.snapshot(items) as Item[])));
    if (entries.some((e) => e.term === "")) { problem = t("dictionary.term_required"); return; }
    saving = true;
    try {
      const next = await setDictionary(entries);
      config.value = next;
      items = toItems(structuredClone(next.dictionary));
      synced = JSON.stringify(next.dictionary);
      toast.ok(t("common.saved"));
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
      <button type="button" class="btn" onclick={add}><Icon name="plus" size={14} />{t("dictionary.add")}</button>
    {/snippet}
  </PageHead>

  {#if items.length}
    <div bind:this={list}>
      <Group help={t("dictionary.snippet_help")}>
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
                  aria-invalid={problem && !item.term.trim() ? "true" : undefined}
                ></textarea>
                {#if isSnippet(item.term)}<span class="tag"><Tag tone="accent">{t("dictionary.snippet")}</Tag></span>{/if}
                <button type="button" class="btn quiet icon-btn" aria-label={t("common.remove")} onclick={() => items!.splice(i, 1)}><Icon name="trash" /></button>
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
  .entry { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; padding: 2px 0; }
  .line { display: flex; align-items: flex-start; gap: 10px; min-width: 0; }
  .second { align-items: center; }
  .term { flex: 1; min-width: 0; min-height: 0; height: 30px; padding: 4px 9px; border-radius: 6px; line-height: 1.5; resize: none; overflow: hidden; }
  .tag { flex: none; margin-top: 5px; }
  .icon-btn { width: 26px; padding: 0; margin: 2px -6px 0 0; color: var(--ink-2); flex: none; }
  .icon-btn:hover { color: var(--ink); }
  .lead { flex: none; }
  .aliases { flex: 1; min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .alias-input { flex: 1; min-width: 90px; height: 24px; padding: 0 4px; border: 0; background: none; font-size: 12px; }
  .hint { flex: none; display: flex; align-items: center; gap: 8px; }
</style>
