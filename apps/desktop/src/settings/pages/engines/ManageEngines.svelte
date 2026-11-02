<script lang="ts">
  /** The role's network engines in one list: tick several to test them one after another or delete them together. */
  import type { Snippet } from "svelte";
  import { errorMessage, profilesPresets, type EngineCheck, type PresetView, type ProviderConfig } from "../../../lib/api";
  import type { Role } from "../../../lib/engines";
  import { removeEngines, type MoveTo } from "../../../lib/engineRemoval";
  import { t } from "../../../lib/i18n";
  import { tip } from "../../../lib/tip";
  import Modal from "../../../lib/ui/Modal.svelte";
  import PickList from "../../../lib/ui/PickList.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";
  import RemovalNote from "./RemovalNote.svelte";

  let { open = $bindable(false), engines, role, isDefault, entry, connect, real, local }: {
    open?: boolean;
    engines: ProviderConfig[];
    role: Role;
    isDefault: (id: string) => boolean;
    /** The engine's ring, name and lines, as its row on the page shows them. */
    entry: Snippet<[ProviderConfig]>;
    /** Test connection: the free models request. */
    connect: (p: ProviderConfig) => Promise<unknown>;
    /** One real request; the page stores and shows the result. */
    real: (p: ProviderConfig) => Promise<EngineCheck>;
    /** A local service: its requests cost nothing, so they go on one press. */
    local: (p: ProviderConfig) => boolean;
  } = $props();

  let selected = $state<string[]>([]);
  let deleting = $state(false);
  let moveTo = $state<MoveTo>("default");
  let presets = $state<PresetView[]>([]);
  let busy = $state(false);
  // A run of tests going one engine after another.
  let run = $state<{ kind: "connect" | "real"; total: number; done: number; passed: number; failed: number } | null>(null);
  let stopping = false;
  let armed = $state(false);
  let armTimer: ReturnType<typeof setTimeout> | undefined;

  const picked = $derived(engines.filter((e) => selected.includes(e.id)));
  const withDefault = $derived(picked.some((e) => isDefault(e.id)));
  // An engine deleted elsewhere leaves the selection.
  $effect(() => {
    const ids = new Set(engines.map((e) => e.id));
    if (selected.some((id) => !ids.has(id))) selected = selected.filter((id) => ids.has(id));
  });

  function disarm() { clearTimeout(armTimer); armed = false; }

  async function test(kind: "connect" | "real") {
    if (kind === "real" && !armed && !picked.every(local)) {
      armed = true;
      armTimer = setTimeout(() => (armed = false), 4000);
      return;
    }
    disarm();
    const list = [...picked];
    stopping = false;
    run = { kind, total: list.length, done: 0, passed: 0, failed: 0 };
    for (const p of list) {
      if (stopping) break;
      let ok = false;
      try {
        if (kind === "real") ok = (await real(p)).ok;
        else { await connect(p); ok = true; }
      } catch { ok = false; }
      run = { ...run!, done: run!.done + 1, passed: run!.passed + (ok ? 1 : 0), failed: run!.failed + (ok ? 0 : 1) };
    }
    run = null;
  }

  async function askDelete() {
    moveTo = "default";
    presets = await profilesPresets().catch(() => []);
    deleting = true;
  }
  async function remove() {
    busy = true;
    try {
      const done = await removeEngines($state.snapshot(picked) as ProviderConfig[], moveTo, presets);
      selected = selected.filter((id) => !done.includes(id));
      toast.ok(done.length > 1 ? t("engines.deleted_n", { n: String(done.length) }) : t("engines.deleted"));
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
    deleting = false;
  }

  function beforeclose(): boolean {
    if (busy) return false;
    if (run) return false;
    if (armed) { disarm(); return false; }
    return true;
  }
  $effect(() => { if (!open) { selected = []; deleting = false; disarm(); } });

  const percent = $derived(run ? Math.round((run.done / Math.max(run.total, 1)) * 100) : 0);
</script>

{#snippet deleteBody()}<RemovalNote engines={picked} {role} {presets} bind:moveTo />{/snippet}
{#snippet deleteActions()}
  <button type="button" class="btn" onclick={() => (deleting = false)} disabled={busy}>{t("common.cancel")}</button>
  <button type="button" class="btn danger" onclick={remove} disabled={busy}>{t("engines.delete")}</button>
{/snippet}

<Modal bind:open size="lg" title={t(`engines.manage_title.${role}`)} {beforeclose}
  question={deleting ? { title: picked.length > 1 ? t("engines.delete_n_title", { n: String(picked.length) }) : t("engines.delete_title"), no: () => { if (!busy) deleting = false; }, body: deleteBody, actions: deleteActions } : null}>
  <div class="gap">
    <PickList items={engines} key={(e) => e.id} bind:selected busy={busy} label={(e) => e.label}>
      {#snippet row(e)}{@render entry(e)}{/snippet}
    </PickList>
  </div>
  {#snippet actions()}
    {#if run}
      <div class="prog" role="status">
        <b>{t(run.kind === "real" ? "engines.batch_real" : "engines.batch_connect", { done: String(Math.min(run.done + 1, run.total)), total: String(run.total) })}</b>
        <span class="bar"><i style:width="{percent}%"></i></span>
        {#if run.passed}<span class="ok">{t("engines.batch_passed", { n: String(run.passed) })}</span>{/if}
        {#if run.failed}<span class="bad">{t("engines.batch_failed", { n: String(run.failed) })}</span>{/if}
      </div>
      <button type="button" class="btn" onclick={() => (stopping = true)}>{t("engines.batch_stop")}</button>
    {:else}
      <span class="count">{picked.length ? t("engines.manage_selected", { n: String(picked.length) }) : t("engines.manage_none")}</span>
      <button type="button" class="btn" disabled={!picked.length} onclick={() => test("connect")}>{t("engines.test_connection")}</button>
      <button type="button" class="btn" class:ask={armed} disabled={!picked.length} use:tip={picked.every(local) ? undefined : t("engines.batch_real_tip")} onclick={() => test("real")}>{armed ? t(picked.length === 1 ? "engines.test_confirm_one" : "engines.test_confirm", { n: String(picked.length) }) : t("engines.batch_real_selected")}</button>
      <span use:tip={withDefault ? t("engines.manage_default_locked") : undefined}>
        <button type="button" class="btn danger" disabled={!picked.length || withDefault} onclick={askDelete}>{t("engines.delete_selected")}</button>
      </span>
      <button type="button" class="btn primary" onclick={() => (open = false)}>{t("flow.manage_done")}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .gap { margin-top: 8px; }
  .count { margin-right: auto; align-self: center; color: var(--ink-2); font-size: var(--text-sm); }
  .prog { margin-right: auto; flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; font-size: var(--text-sm); color: var(--ink-2); }
  .prog b { color: var(--ink); font-weight: 600; white-space: nowrap; }
  .bar { flex: 1; max-width: 240px; height: 6px; border-radius: 3px; background: var(--sunken); overflow: hidden; }
  .bar i { display: block; height: 100%; border-radius: 3px; background: var(--accent); transition: width .2s; }
  .ok { color: var(--ok); white-space: nowrap; }
  .bad { color: var(--live); white-space: nowrap; }
  .btn.ask { background: var(--live); border-color: var(--live); color: #fff; }
</style>
