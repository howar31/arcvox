<script lang="ts">
  /** Every profile in one list: tick several and delete them together; rename, reset, make default or delete one in its row. */
  import { tip } from "../../../lib/tip";
  import { tick } from "svelte";
  import { deleteProfile, errorMessage, resetProfile, setDefaultProfile, upsertProfile, type PresetView, type Profile } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { isImeKey } from "../../../lib/ime";
  import { config } from "../../../lib/store.svelte";
  import Modal from "../../../lib/ui/Modal.svelte";
  import PickList from "../../../lib/ui/PickList.svelte";
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

{#snippet resetBody()}<p class="ask">{resetKnown ? t("flow.reset_body", { preset: presetName(resetPreset) }) : t("flow.reset_missing_body")}</p>{/snippet}
{#snippet resetActions()}
  <button type="button" class="btn" onclick={() => (resetting = null)}>{t("common.cancel")}</button>
  <button type="button" class="btn primary" onclick={reset}>{resetKnown ? t("flow.reset") : t("flow.reset_to_casual")}</button>
{/snippet}
{#snippet deleteBody()}<p class="ask">{t("flow.manage_delete_body", { names })}</p>{/snippet}
{#snippet deleteActions()}
  <button type="button" class="btn" onclick={() => (confirming = [])}>{t("common.cancel")}</button>
  <button type="button" class="btn danger" onclick={remove}>{t("profiles.delete")}</button>
{/snippet}

<Modal bind:open size="lg" title={t("flow.manage_title")} beforeclose={() => !busy}
  question={resetTarget ? { title: resetKnown ? t("flow.reset_title", { name: resetTarget.name }) : t("flow.reset_missing_title", { name: resetTarget.name }), no: () => (resetting = null), body: resetBody, actions: resetActions }
    : confirming.length ? { title: t("flow.manage_delete_title", { count: String(confirming.length) }), no: () => (confirming = []), body: deleteBody, actions: deleteActions } : null}>
    <div class="list-gap"><PickList items={profiles} key={(p) => p.id} bind:selected {busy} label={(p) => p.name} locked={(p) => (p.is_default ? t("flow.manage_default_locked") : null)}>
      {#snippet row(p)}
        <div class="item">
          <div class="text">
            {#if renaming === p.id}
              <input
                type="text" class="name" bind:this={nameField} bind:value={newName} aria-label={t("profiles.name")}
                onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) finishRename(); else if (e.key === "Escape") { renaming = null; e.stopPropagation(); } }}
                onblur={finishRename}
              />
            {:else}
              <span class="line"><span class="label">{p.name}</span>{#if p.is_default}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}{#if p.preset}{#if presets.some((x) => x.id === p.preset)}<Tag title={t("flow.manage_preset_hint", { preset: presetName(p.preset) })}>{presetName(p.preset)}</Tag>{:else}<Tag tone="warn" title={t("flow.reset_missing_body")}>{t("flow.manage_preset_gone")}</Tag>{/if}{/if}</span>
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
      {/snippet}
    </PickList></div>
  {#snippet actions()}
      <span class="count">{selected.length ? t("flow.manage_selected", { count: String(selected.length) }) : t("flow.manage_none_selected")}</span>
      <button type="button" class="btn danger" disabled={!selected.length || busy} onclick={() => (confirming = [...selected])}>{t("flow.manage_delete_selected")}</button>
      <button type="button" class="btn primary" disabled={busy} onclick={() => (open = false)}>{t("flow.manage_done")}</button>
  {/snippet}
</Modal>

<style>
  .ask { margin: 0; color: var(--ink-2); }
  .list-gap { margin-top: 8px; }
  .item { display: flex; align-items: center; gap: 14px; min-width: 0; }
  .text { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .line { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .label { font-weight: 600; font-size: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .where { color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name { width: 100%; height: 30px; }
  .acts { display: flex; gap: 0; flex: none; }
  .sm { height: 30px; padding: 0 9px; font-size: var(--text-sm); }
  .count { margin-right: auto; align-self: center; color: var(--ink-2); font-size: var(--text-sm); }
</style>
