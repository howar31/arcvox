<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import {
    errorMessage, formatBytes, modelsCancel, modelsDelete, modelsDir, modelsDownload, modelsDownloadRecommended, modelsImport, modelsInstalled,
    modelsManifest, modelsManifestNow, onModelDownloadProgress, probeProvider, reveal, upsertProvider,
    type DownloadProgress, type InstalledModel, type ManifestView, type ModelEntry, type ProviderConfig,
  } from "../../lib/api";
  import { engineFromModel, enginesOf, hasRole, recommendedMissing, runtimeFor, unusedModels, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { modelDescription, modelLabel, usedBy } from "../../lib/labels";
  import { rowStatus } from "../../lib/models.svelte";
  import { engineMeta } from "../../lib/profileEdit";
  import { baseName, describeProbe, NATIVE_FM } from "../../lib/providerForm";
  import { engineDescription, engineRecommended } from "../../lib/stages";
  import { guard } from "../../lib/guard.svelte";
  import { go } from "../../lib/route";
  import { config } from "../../lib/store.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import EngineForm from "./EngineForm.svelte";

  let { arg }: { arg: string | null } = $props();
  const role = $derived<Role>(arg === "llm" ? "llm" : "asr");

  let manifest = $state<ManifestView | null>(null);
  let installed = $state<InstalledModel[]>([]);
  let dir = $state("");
  let progress = $state<Record<string, DownloadProgress>>({});
  let refreshing = $state(false);
  // Per-row test result, keyed by provider id.
  let rowProbes = $state<Record<string, { state: "wait" | "ok" | "bad"; text: string }>>({});
  let formOpen = $state(false);
  let formInitial = $state<ProviderConfig | null>(null);
  // The installed model waiting for the delete confirmation.
  let deleting = $state<InstalledModel | null>(null);

  const roles = $derived([
    { value: "asr" as Role, label: t("engines.role.asr") },
    { value: "llm" as Role, label: t("engines.role.llm") },
  ]);
  const providers = $derived(config.value?.providers ?? []);
  const engines = $derived(enginesOf(config.value, role));
  const installedIds = $derived(new Set(installed.map((m) => m.id)));
  const installedPaths = $derived(new Set(installed.map((m) => m.path)));
  const catalogue = $derived((manifest?.models ?? []).filter((m) => m.role === role));
  const unused = $derived(unusedModels(installed, providers, role));
  const imported = $derived(installed.filter((m) => m.custom && m.runtime === runtimeFor(role)));
  const missingRecommended = $derived(recommendedMissing(manifest, installed));
  const busy = $derived(Object.values(progress).some((p) => !p.done));

  async function refreshInstalled() {
    try { installed = await modelsInstalled(); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function refreshManifest() {
    refreshing = true;
    try {
      // Render the local catalogue first; the online one can take until its fetch times out.
      if (!manifest) manifest = await modelsManifestNow();
      manifest = await modelsManifest();
    } catch (e) { toast.error(errorMessage(e)); }
    refreshing = false;
  }
  onMount(() => {
    refreshManifest();
    refreshInstalled();
    modelsDir().then((d) => { dir = d; }).catch((e) => toast.error(errorMessage(e)));
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onModelDownloadProgress((p) => {
      progress = { ...progress, [p.id]: p };
      if (p.done && !p.error) refreshInstalled();
    }).then((u) => { if (disposed) u(); else unlisten = u; });
    return () => { disposed = true; unlisten?.(); };
  });

  // Engines
  const describe = (p: ProviderConfig) => engineDescription(p, manifest);
  function usersLine(p: ProviderConfig): string {
    const users = usedBy(config.value, p.id);
    if (users.length === 0) return t("engines.not_in_use");
    const names = users.map((u) => (u.isDefault ? t("engines.default_user", { name: u.name }) : u.name)).join(t("engines.list_sep"));
    return t("engines.in_use", { names });
  }
  const recommended = (p: ProviderConfig) => engineRecommended(p, manifest);

  async function test(p: ProviderConfig) {
    rowProbes = { ...rowProbes, [p.id]: { state: "wait", text: t("engines.testing") } };
    let next: { state: "ok" | "bad"; text: string };
    try {
      const r = await probeProvider($state.snapshot(p) as ProviderConfig);
      next = { state: r.ok ? "ok" : "bad", text: describeProbe(r, t) };
    } catch (e) {
      next = { state: "bad", text: `${t("engines.probe_failed")}: ${errorMessage(e)}` };
    }
    rowProbes = { ...rowProbes, [p.id]: next };
  }
  function openForm(p: ProviderConfig | null) {
    formInitial = p;
    formOpen = true;
  }

  // Models
  const isUnused = (id: string): boolean => unused.some((m) => m.id === id);

  async function createEngine(id: string) {
    const m = installed.find((x) => x.id === id);
    if (!m) return;
    try {
      await upsertProvider(engineFromModel(m, modelLabel(manifest, m), providers.map((p) => p.id)));
      toast.ok(t("engines.created"));
    } catch (e) { toast.error(errorMessage(e)); }
  }
  function download(id: string) {
    const { [id]: _, ...rest } = progress;
    progress = rest;
    modelsDownload(id).catch((e) => toast.error(errorMessage(e)));
  }
  function downloadRecommended() {
    modelsDownloadRecommended().catch((e) => toast.error(errorMessage(e)));
  }
  async function cancel(id: string) {
    try { await modelsCancel(id); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function removeModel() {
    const m = deleting;
    deleting = null;
    if (!m) return;
    try { await modelsDelete(m.id); toast.ok(t("engines.model_deleted")); } catch (e) { toast.error(errorMessage(e)); }
    await refreshInstalled();
  }
  async function importFile() {
    try {
      const path = await openDialog({ multiple: false, directory: false, filters: [{ name: "Model", extensions: ["bin", "gguf"] }] });
      if (!path) return;
      await modelsImport(path);
      toast.ok(t("engines.imported"));
      await refreshInstalled();
    } catch (e) { toast.error(errorMessage(e)); }
  }
  const percent = (received: number, total: number): number => (total > 0 ? Math.min(100, Math.floor((received / total) * 100)) : 0);
  const entryMeta = (m: ModelEntry): string =>
    t("engines.model_meta", { size: formatBytes(m.size_bytes), license: m.license, languages: m.languages.join(t("engines.list_sep")) });
</script>

<PageHead title={t("nav.engines")} description={t("engines.description")}>
  {#snippet actions()}
    <button type="button" class="btn primary" onclick={() => openForm(null)}><Icon name="plus" size={14} />{t("engines.add")}</button>
  {/snippet}
</PageHead>

<div class="toolbar">
  <Segmented options={roles} bind:value={() => role, (v) => guard.request(() => go("engines", v))} label={t("engines.role")} />
  {#if missingRecommended.length > 0}
    <button type="button" class="btn quiet" disabled={busy} onclick={downloadRecommended}><Icon name="download" />{t("engines.download_recommended")}</button>
  {/if}
</div>

<Group title={t("engines.configured")}>
  {#each engines as p (p.id)}
    {@const meta = engineMeta(p, installedIds, installedPaths)}
    {@const probe = rowProbes[p.id]}
    <Row>
      {#snippet leading()}<Ring tone={usedBy(config.value, p.id).length > 0 ? "accent" : "off"} />{/snippet}
      <div class="info">
        <div class="title">
          <span class="name">{p.label}</span>
          <Tag>{t("engines.tag." + p.kind)}</Tag>
          {#if recommended(p)}<Tag tone="accent">{t("engines.recommended")}</Tag>{/if}
          {#if !hasRole(p)}<Tag tone="warn">{t("engines.no_role")}</Tag>{/if}
          {#if meta.kind === "local" && !meta.ready && p.model_id}<Tag tone="warn">{t("engines.not_downloaded")}</Tag>
          {:else if meta.kind === "local" && !meta.ready && p.model_path}<Tag tone="warn">{t("engines.file_missing")}</Tag>{/if}
        </div>
        <small>{describe(p)}</small>
        <small>{usersLine(p)}</small>
        {#if probe}<small class="probe {probe.state}" role="status">{probe.text}</small>{/if}
      </div>
      <button type="button" class="btn quiet" onclick={() => test(p)} disabled={probe?.state === "wait"}>{t("engines.test")}</button>
      <button type="button" class="btn quiet" onclick={() => openForm(p)}>{t("common.edit")}</button>
    </Row>
  {:else}
    <Empty text={t("engines.none." + role)}>
      {#snippet action()}<button type="button" class="btn" onclick={() => openForm(null)}>{t("engines.add")}</button>{/snippet}
    </Empty>
  {/each}
</Group>

{#snippet modelState(id: string)}
  <Tag tone="ok">{t("engines.installed")}</Tag>
  {#if isUnused(id)}<button type="button" class="btn quiet" onclick={() => createEngine(id)}>{t("engines.create")}</button>{/if}
  <button type="button" class="btn quiet danger" onclick={() => { deleting = installed.find((m) => m.id === id) ?? null; }}>{t("engines.delete_model")}</button>
{/snippet}

<Group title={t("engines.catalogue")}>
  {#if !manifest}
    <Row><span class="sub">{t("common.loading")}</span></Row>
  {/if}
  {#each catalogue as m (m.id)}
    {@const s = rowStatus(m, installed, progress)}
    <Row>
      <div class="info">
        <div class="title">
          <span class="name">{m.label}</span>
          {#if m.recommended}<Tag tone="accent">{t("engines.recommended")}</Tag>{/if}
        </div>
        <small class="line" title={entryMeta(m)}>{entryMeta(m)}</small>
        {#if modelDescription(m)}<small>{modelDescription(m)}</small>{/if}
        {#if s.kind === "error"}<small class="probe bad">{s.message}</small>{/if}
      </div>
      {#if s.kind === "missing"}
        <button type="button" class="btn" onclick={() => download(m.id)}><Icon name="download" />{t("engines.download")}</button>
      {:else if s.kind === "downloading"}
        {@const pct = percent(s.received, s.total)}
        <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={m.label}><i style:width="{pct}%"></i></div>
        <span class="sub num pct">{pct}%</span>
        <button type="button" class="btn quiet" onclick={() => cancel(m.id)}>{t("common.cancel")}</button>
      {:else if s.kind === "installed"}
        {@render modelState(m.id)}
      {:else}
        <button type="button" class="btn" onclick={() => download(m.id)}>{t("engines.retry")}</button>
      {/if}
    </Row>
  {/each}
</Group>

{#if imported.length > 0}
  <Group title={t("engines.imported_title")}>
    {#each imported as m (m.path)}
      <Row>
        <div class="info">
          <div class="title"><span class="name">{baseName(m.path)}</span></div>
          <small>{formatBytes(m.size_bytes)}</small>
        </div>
        {@render modelState(m.id)}
      </Row>
    {/each}
  </Group>
{/if}

<div class="foot">
  <p class="help">
    {#if dir}{t("engines.dir", { dir })}{/if}
    <button type="button" class="link" onclick={() => reveal(dir).catch((e) => toast.error(errorMessage(e)))} disabled={!dir}>{t("engines.reveal")}</button>
    <button type="button" class="link" onclick={importFile}>{t("engines.import")}</button>
  </p>
  <p class="help">
    {#if manifest}{t(`engines.source.${manifest.source}`)}{/if}
    <button type="button" class="link" onclick={refreshManifest} disabled={refreshing}>{t("engines.refresh")}</button>
  </p>
</div>

<Modal bind:open={() => deleting !== null, (v) => { if (!v) deleting = null; }} title={t("engines.delete_model_title")}>
  {#if deleting}<p>{t("engines.delete_model_body", { name: modelLabel(manifest, deleting), size: formatBytes(deleting.size_bytes) })}</p>{/if}
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (deleting = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={removeModel}>{t("engines.delete_model")}</button>
  {/snippet}
</Modal>

{#if formOpen}
  <EngineForm bind:open={formOpen} initial={formInitial} {role} {installed} {manifest} />
{/if}

<style>
  .info { flex: 1; min-width: 0; }
  .toolbar { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 8px; }
  .toolbar .btn.quiet { margin-right: -8px; }
  /* Tags move to a second line before the name is shortened. */
  .title { display: flex; align-items: center; flex-wrap: wrap; gap: 2px 6px; min-width: 0; }
  .title .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .info small { display: block; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .info small.line { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .probe.ok { color: var(--ok); }
  .probe.bad { color: var(--live); }
  .bar { width: 96px; height: 4px; border-radius: 2px; background: var(--sunken); overflow: hidden; flex: none; }
  .bar i { display: block; height: 100%; background: var(--accent); transition: width .2s; }
  .pct { width: 38px; text-align: right; flex: none; }
  .foot { margin-top: 8px; }
  .foot .help { margin-top: 4px; }
  .link { border: 0; background: none; padding: 0; margin-left: 6px; color: var(--accent); font-size: var(--text-sm); cursor: default; }
  .link:hover { text-decoration: underline; }
  .link:disabled { opacity: .5; }
</style>
