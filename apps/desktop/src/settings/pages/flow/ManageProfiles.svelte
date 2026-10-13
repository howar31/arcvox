<script lang="ts">
  /** Every profile in one list: tick several and delete them together; rename, reset, make default or delete one in its row. */
  import { tick } from "svelte";
  import { deleteProfile, errorMessage, resetProfile, setDefaultProfile, upsertProfile, type PresetView, type Profile } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { isImeKey } from "../../../lib/ime";
  import { config } from "../../../lib/store.svelte";
  import Modal from "../../../lib/ui/Modal.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";

  let { open = $bindable(false), profiles, applies, presets }: {
    open?: boolean;
    profiles: Profile[];
    applies: (p: Profile) => string;
    /** The built-in presets of this version; a profile whose preset is missing is offered the casual one. */
    presets: PresetView[];
  } = $props();

  let selected = $state<string[]>([]);
  // The ids about to be deleted once the user confirms; empty when nothing is asked.
  let confirming = $state<string[]>([]);
  let renaming = $state<string | null>(null);
  // The profile about to be reset once the user confirms.
  let resetting = $state<string | null>(null);
  let newName = $state("");
  let nameField: HTMLInputElement | undefined = $state();
  let busy = $state(false);

  const byId = (id: string) => profiles.find((p) => p.id === id);
  const names = $derived(confirming.map((id) => byId(id)?.name ?? id).join(t("home.list_sep")));
  // The reset goes back to the profile's own preset when this version still has it, else it offers the casual one.
  const resetTarget = $derived(resetting ? byId(resetting) : undefined);
  const resetPreset = $derived(resetTarget?.preset ?? "casual");
  const resetKnown = $derived(presets.some((x) => x.id === resetPreset));
  const presetName = (id: string) => t(`profiles.preset.${id}`);

  $effect(() => {
    if (!open) { selected = []; confirming = []; resetting = null; renaming = null; }
  });

  function toggle(p: Profile) {
    if (p.is_default) return;
    selected = selected.includes(p.id) ? selected.filter((x) => x !== p.id) : [...selected, p.id];
  }

  async function startRename(p: Profile) {
    renaming = p.id;
    newName = p.name;
    await tick();
    nameField?.focus();
    nameField?.select();
  }
  async function finishRename() {
    const p = renaming ? byId(renaming) : undefined;
    renaming = null;
    const name = newName.trim();
    if (!p || !name || name === p.name) return;
    busy = true;
    try {
      config.value = await upsertProfile({ ...$state.snapshot(p), name } as Profile);
    } catch (e) { toast.error(errorMessage(e)); } finally { busy = false; }
  }

  async function makeDefault(p: Profile) {
    busy = true;
    try {
      config.value = await setDefaultProfile(p.id);
      selected = selected.filter((x) => x !== p.id);
      toast.ok(t("profiles.made_default"));
    } catch (e) { toast.error(errorMessage(e)); } finally { busy = false; }
  }

  /** Puts the profile's settings back to its preset, or to the casual one when its own is gone. */
  async function reset() {
    const p = resetTarget;
    const toCasual = !resetKnown;
    resetting = null;
    if (!p) return;
    busy = true;
    try {
      config.value = await resetProfile(p.id, toCasual ? "casual" : undefined);
      toast.ok(t("flow.reset_done", { preset: presetName(toCasual ? "casual" : resetPreset) }));
    } catch (e) { toast.error(errorMessage(e)); } finally { busy = false; }
  }

  async function remove() {
    const ids = confirming;
    confirming = [];
    busy = true;
    let done = 0;
    try {
      for (const id of ids) {
        config.value = await deleteProfile(id);
        done++;
      }
      selected = selected.filter((x) => !ids.includes(x));
      toast.ok(done === 1 ? t("profiles.deleted") : t("flow.manage_deleted", { count: String(done) }));
    } catch (e) {
      selected = selected.filter((x) => !ids.slice(0, done).includes(x));
      toast.error(errorMessage(e));
    } finally { busy = false; }
  }
</script>

<Modal bind:open title={resetTarget ? (resetKnown ? t("flow.reset_title", { name: resetTarget.name }) : t("flow.reset_missing_title", { name: resetTarget.name })) : confirming.length ? t("flow.manage_delete_title", { count: String(confirming.length) }) : t("flow.manage_title")} beforeclose={() => { if (resetting) { resetting = null; return false; } if (confirming.length) { confirming = []; return false; } return !busy; }}>
  {#if resetTarget}
    <p class="ask">{resetKnown ? t("flow.reset_body", { preset: presetName(resetPreset) }) : t("flow.reset_missing_body")}</p>
  {:else if confirming.length}
    <p class="ask">{t("flow.manage_delete_body", { names })}</p>
  {:else}
    <div class="list" class:busy inert={busy} aria-busy={busy}>
      {#each profiles as p (p.id)}
        {@const on = selected.includes(p.id)}
        <!-- The name and its line pick the row too; the box stays the control for the keyboard. -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="row" class:on class:pickable={!p.is_default} onclick={(e) => { if (!(e.target as HTMLElement).closest("button, input")) toggle(p); }}>
          <button type="button" class="cb" class:on role="checkbox" aria-checked={on} aria-label={p.name} disabled={p.is_default} title={p.is_default ? t("flow.manage_default_locked") : undefined} onclick={() => toggle(p)}></button>
          <div class="text">
            {#if renaming === p.id}
              <input
                type="text" class="name" bind:this={nameField} bind:value={newName} aria-label={t("profiles.name")}
                onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) finishRename(); else if (e.key === "Escape") { renaming = null; e.stopPropagation(); } }}
                onblur={finishRename}
              />
            {:else}
              <span class="line"><span class="label">{p.name}</span>{#if p.is_default}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}</span>
            {/if}
            <span class="where">{applies(p)}</span>
          </div>
          <div class="acts">
            <button type="button" class="btn quiet sm" onclick={() => startRename(p)}>{t("flow.rename")}</button>
            <button type="button" class="btn quiet sm" onclick={() => (resetting = p.id)}>{t("flow.reset")}</button>
            {#if !p.is_default}
              <button type="button" class="btn quiet sm" onclick={() => makeDefault(p)}>{t("profiles.make_default")}</button>
              <button type="button" class="btn quiet sm danger" onclick={() => (confirming = [p.id])}>{t("profiles.delete")}</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
  {#snippet actions()}
    {#if resetTarget}
      <button type="button" class="btn" onclick={() => (resetting = null)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={reset}>{resetKnown ? t("flow.reset") : t("flow.reset_to_casual")}</button>
    {:else if confirming.length}
      <button type="button" class="btn" onclick={() => (confirming = [])}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={remove}>{t("profiles.delete")}</button>
    {:else}
      <span class="count">{selected.length ? t("flow.manage_selected", { count: String(selected.length) }) : t("flow.manage_none_selected")}</span>
      <button type="button" class="btn danger" disabled={!selected.length || busy} onclick={() => (confirming = [...selected])}>{t("flow.manage_delete_selected")}</button>
      <button type="button" class="btn primary" disabled={busy} onclick={() => (open = false)}>{t("flow.manage_done")}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .ask { margin: 0; color: var(--ink-2); }
  .list { margin-top: 8px; border: 1px solid var(--line); border-radius: var(--radius-group); background: var(--surface); color: var(--ink); }
  .list.busy { opacity: .55; }
  .row { display: grid; grid-template-columns: 18px minmax(0, 1fr) auto; gap: 14px; align-items: center; padding: 12px 16px; border-top: 1px solid var(--line); }
  .row:first-child { border-top: 0; }
  .row.on { background: var(--accent-soft); }
  .row.pickable:hover { background: color-mix(in srgb, var(--sunken) 60%, transparent); }
  .row.pickable.on:hover { background: var(--accent-soft); }
  .cb { width: 18px; height: 18px; padding: 0; border: 1.5px solid var(--line-strong); border-radius: 5px; background: var(--surface); position: relative; cursor: default; }
  .cb.on { background: var(--accent); border-color: var(--accent); }
  .cb.on::after { content: ""; position: absolute; left: 5px; top: 1px; width: 5px; height: 10px; border-right: 2px solid var(--accent-ink); border-bottom: 2px solid var(--accent-ink); transform: rotate(45deg); }
  .cb:disabled { opacity: .35; }
  .text { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .line { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .label { font-weight: 600; font-size: 15px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where { color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name { width: 100%; height: 30px; }
  .acts { display: flex; gap: 0; flex: none; }
  .sm { height: 30px; padding: 0 9px; font-size: var(--text-sm); }
  .count { margin-right: auto; align-self: center; color: var(--ink-2); font-size: var(--text-sm); }
</style>
