<script lang="ts">
  import { tip } from "../../lib/tip";
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import {
    errorMessage, formatBytes, modelsCancel, modelsCustomDir, modelsDelete, modelsDirGet, modelsDirSet, modelsDownload, modelsDownloadRecommended, modelsImport,
    modelsInstalled, modelsManifest, modelsManifestNow, nativeInstallLanguage, nativeStatus, onModelDownloadProgress, onNativeInstallProgress, openFolder,
    probeProvider, setDefaultEngine, type DownloadProgress, type InstalledModel, type ManifestView, type ModelEntry, type ModelsDir, type NativeStatusView, type ProviderConfig,
  } from "../../lib/api";
  import { encoderMissing, enginesOf, entryBytes, factsLine, hasRole, modelServes, recommendedMissing, runnable, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { defaultProfile, modelDescription, modelLabel, namedBy, usedBy } from "../../lib/labels";
  import { describeProbe, NATIVE_DICTATION } from "../../lib/providerForm";
  import { usableWith } from "../../lib/resolve";
  import { engineDescription } from "../../lib/stages";
  import { guard } from "../../lib/guard.svelte";
  import { go } from "../../lib/route";
  import { config } from "../../lib/store.svelte";
  import { rememberFilter, rememberFold, storedFilter, storedFold, type ModelFilter } from "../../lib/engineListMemory";
  import Empty from "../../lib/ui/Empty.svelte";
  import Fold from "../../lib/ui/Fold.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import InfoTip from "../../lib/ui/InfoTip.svelte";
  import PageLink from "../../lib/ui/PageLink.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import EngineForm from "./EngineForm.svelte";
  import General from "./General.svelte";

  let { arg }: { arg: string | null } = $props();
  const role = $derived<Role>(arg === "llm" ? "llm" : "asr");

  let manifest = $state<ManifestView | null>(null);
  let installed = $state<InstalledModel[]>([]);
  // The files on disk have been read once; before that no engine is called gone.
  let installedKnown = $state(false);
  // The folder of the model files. Changing it moves them at once.
  let folder = $state<ModelsDir | null>(null);
  let moving = $state(false);
  let progress = $state<Record<string, DownloadProgress>>({});
  let refreshing = $state(false);
  // Per-row test result, keyed by engine id, with the configuration it was
  // made for: an engine that is deleted or changed loses its result, also
  // when a new engine takes the same id.
  let rowProbes = $state<Record<string, { state: "wait" | "ok" | "bad"; text: string; of: string }>>({});
  $effect(() => {
    const providers = config.value?.providers ?? [];
    const stale = Object.entries(rowProbes)
      .filter(([id, probe]) => JSON.stringify(providers.find((p) => p.id === id) ?? null) !== probe.of)
      .map(([id]) => id);
    if (stale.length === 0) return;
    const next = { ...rowProbes };
    for (const id of stale) delete next[id];
    rowProbes = next;
  });
  // The platform services' status, keyed by engine id.
  let nativeStates = $state<Record<string, NativeStatusView>>({});
  let installing = $state(false);
  let installFraction = $state(0);
  let formOpen = $state(false);
  let formInitial = $state<ProviderConfig | null>(null);
  // The installed model waiting for the delete confirmation.
  let deleting = $state<InstalledModel | null>(null);

  // A tab carries the problem mark while a ring in it is red.
  const roles = $derived((["asr", "llm"] as Role[]).map((r) => {
    const level = roleProblem(r);
    return { value: r, label: t(`engines.role.${r}`), problem: level ? t("engines.tab_problem") : undefined, level: level ?? undefined };
  }));
  const engines = $derived(enginesOf(config.value, role));
  const builtins = $derived(engines.filter((p) => p.kind === "native"));
  const externals = $derived(engines.filter((p) => p.kind === "http"));
  const missingRecommended = $derived(recommendedMissing(manifest, installed));
  const busy = $derived(Object.values(progress).some((p) => !p.done));
  const defaultHint = $derived(defaultProfile(config.value)?.language_hint ?? null);
  const defaultId = $derived(config.value?.default_engines[role] ?? "");
  /** Which engines can run now: a local one by its file, a platform service by its status, a network one at its word. */
  const usable = $derived.by(() => {
    const ok = runnable(installed);
    const files = usableWith(installedKnown ? new Set(ok.map((m) => m.id)) : null, new Set(ok.map((m) => m.path)));
    return (p: ProviderConfig) => (p.kind === "native" ? (nativeStates[p.id]?.status ?? "ready") === "ready" : files(p));
  });
  /** Whether the engine can become the role's default now: it serves the role and can run. */
  const canBeDefault = (p: ProviderConfig): boolean =>
    p.id !== defaultId && (role === "asr" ? p.capabilities.transcribe : p.capabilities.chat) && usable(p);
  async function makeDefault(p: ProviderConfig) {
    try {
      config.value = await setDefaultEngine(role, p.id);
      toast.ok(t("common.saved"));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

  /**
   * One row per local model of the role: an installed file (catalogue or imported), a catalogue entry
   * still to download, or an engine whose file is gone. Installed and downloading rows come first.
   */
  type LocalRow = {
    id: string; label: string; desc: string; meta: string;
    engine: ProviderConfig | null; model: InstalledModel | null;
    state: "installed" | "downloading" | "missing" | "error" | "gone"; received: number; total: number; error: string;
  };
  function rowsFor(role: Role): LocalRow[] {
    if (!manifest || !installedKnown) return [];
    const engines = enginesOf(config.value, role);
    const providers = config.value?.providers ?? [];
    const engineOf = (id: string) => providers.find((p) => p.id === id && p.kind === "embedded") ?? null;
    const rows: LocalRow[] = [];
    const seen = new Set<string>();
    for (const m of installed.filter((x) => modelServes(x, role))) {
      const entry = manifest?.models.find((e) => e.id === m.id) ?? null;
      seen.add(m.id);
      rows.push({
        id: m.id, label: modelLabel(manifest, m), engine: engineOf(m.id), model: m, state: "installed", received: 0, total: 0, error: "",
        desc: entry ? modelDescription(entry) : m.capabilities.transcribe && m.capabilities.chat ? t("engines.both_roles") : "",
        meta: entry ? entryMeta(entry) : [formatBytes(m.size_bytes), factsLine(m)].filter(Boolean).join(t("engines.fact_sep")),
      });
    }
    for (const e of (manifest?.models ?? []).filter((x) => x.role === role)) {
      if (seen.has(e.id)) continue;
      seen.add(e.id);
      const p = progress[e.id];
      const state = p && !p.done ? "downloading" : p?.error ? "error" : "missing";
      rows.push({ id: e.id, label: e.label, desc: modelDescription(e), meta: entryMeta(e), engine: engineOf(e.id), model: null, state, received: p?.received ?? 0, total: p?.total ?? 0, error: p?.error ?? "" });
    }
    for (const p of engines.filter((x) => x.kind === "embedded" && !seen.has(x.id))) {
      rows.push({ id: p.id, label: p.label, desc: engineDescription(p, manifest), meta: "", engine: p, model: null, state: "gone", received: 0, total: 0, error: "" });
    }
    const rank = (r: LocalRow) => (r.state === "installed" ? 0 : r.state === "downloading" ? 1 : r.state === "gone" ? 2 : 3);
    return rows.sort((a, b) => rank(a) - rank(b));
  }
  const locals = $derived(rowsFor(role));

  // The filter keeps installed or own files only; a row with a red ring stays in every filter.
  // Catalogue models nobody has asked for yet sit in the fold at the end of the full list.
  let filter = $state<ModelFilter>(storedFilter());
  $effect(() => { rememberFilter(filter); });
  const isOwn = (r: LocalRow): boolean => r.model?.custom ?? (!!r.engine?.model_path && !r.engine.model_id);
  const toDownload = (r: LocalRow): boolean => r.state === "missing" && !r.engine;
  const inFilter = (r: LocalRow): boolean => filter === "all" || (filter === "own" ? isOwn(r) : r.state === "installed" || r.state === "downloading");
  // A search looks through every engine on the tab, the fold included, and shows only what matches.
  let query = $state("");
  const q = $derived(query.trim().toLowerCase());
  const matches = (...texts: string[]): boolean => texts.some((x) => x.toLowerCase().includes(q));
  const shownRows = $derived(
    q ? locals.filter((r) => inFilter(r) && matches(r.label, r.id, r.meta, r.desc))
      : locals.filter((r) => !toDownload(r) && (inFilter(r) || rowLevel(r) !== null)),
  );
  const foldRows = $derived(filter === "all" && !q ? locals.filter(toDownload) : []);
  const shownBuiltins = $derived(q ? builtins.filter((p) => matches(p.label, engineDescription(p, manifest))) : builtins);
  const shownExternals = $derived(q ? externals.filter((p) => matches(p.label, engineDescription(p, manifest))) : externals);
  // Open while a recommended model is missing, until the user opens or shuts it.
  let foldChoice = $state<boolean | null>(storedFold());
  const foldOpen = $derived(foldChoice ?? foldRows.some((r) => recommended(r.id)));
  const filters = $derived([
    { value: "all" as ModelFilter, label: t("engines.filter.all"), count: locals.length },
    { value: "installed" as ModelFilter, label: t("engines.filter.installed"), count: locals.filter((r) => r.state === "installed").length },
    { value: "own" as ModelFilter, label: t("engines.filter.own"), count: locals.filter(isOwn).length },
  ]);

  async function refreshInstalled() {
    try { installed = await modelsInstalled(); installedKnown = true; } catch (e) { toast.error(errorMessage(e)); }
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
  // The catalogue and the files on disk, looked at again; the engines follow on the way.
  async function refreshAll() {
    await Promise.all([refreshManifest(), refreshInstalled()]);
  }
  onMount(() => {
    refreshAll();
    loadFolder();
    let unlistenModels: (() => void) | undefined;
    let unlistenNative: (() => void) | undefined;
    let disposed = false;
    onModelDownloadProgress((p) => {
      progress = { ...progress, [p.id]: p };
      if (p.done && !p.error) refreshInstalled();
    }).then((u) => { if (disposed) u(); else unlistenModels = u; });
    onNativeInstallProgress((f) => (installFraction = f)).then((u) => { if (disposed) u(); else unlistenNative = u; });
    return () => { disposed = true; unlistenModels?.(); unlistenNative?.(); };
  });

  // The platform services report their status as the page opens, for both tabs' marks.
  $effect(() => {
    for (const p of (config.value?.providers ?? []).filter((x) => x.kind === "native")) {
      if (!p.native_id || nativeStates[p.id]) continue;
      nativeStatus(p.native_id, defaultHint)
        .then((s) => { nativeStates = { ...nativeStates, [p.id]: s }; })
        .catch((e) => { nativeStates = { ...nativeStates, [p.id]: { status: "failed", reason: errorMessage(e) } }; });
    }
  });
  const STATUSES = ["ready", "needs_download", "locale_unsupported", "os_too_old", "not_eligible", "intelligence_off", "model_not_ready", "failed"];
  async function installLanguage(p: ProviderConfig) {
    installing = true;
    installFraction = 0;
    try { await nativeInstallLanguage(defaultHint); }
    catch (e) { const m = errorMessage(e); toast.error(STATUSES.includes(m) ? t("engines.native." + m) : m); }
    installing = false;
    const { [p.id]: _, ...rest } = nativeStates;
    nativeStates = rest;
  }

  /**
   * The ring before an engine says one thing: in use by a profile and working (accent), ready and
   * waiting (green), not usable yet and nobody depends on it (grey), or a problem to deal with: a
   * profile depends on it and it cannot work, a download or a test failed, a file is gone. The problem
   * is red when it is a default engine, so dictation with the default fails, and orange otherwise,
   * since a profile then runs on the default meanwhile.
   */
  type Level = "live" | "warn";
  type Light = { tone: "accent" | "ok" | "off" | Level; title: string };
  const isDefaultEngine = (id: string): boolean => id === config.value?.default_engines.asr || id === config.value?.default_engines.llm;
  const levelOf = (tone: Light["tone"]): Level | null => (tone === "live" || tone === "warn" ? tone : null);
  /** The problem level of a local model's ring; null when it has none. */
  function rowLevel(r: LocalRow): Level | null {
    const deaf = r.model !== null && encoderMissing(r.model);
    return levelOf(light(r.id, r.state === "installed" && !deaf, r.state === "error" || r.state === "gone" || deaf || failed(r.id)).tone);
  }
  /** The worst problem level of any ring on the role's tab: a local model, a platform service or a network engine. */
  function roleProblem(r: Role): Level | null {
    const levels = [
      ...rowsFor(r).map(rowLevel),
      ...enginesOf(config.value, r).map((p) => {
        if (p.kind === "native") {
          const s = nativeStates[p.id]?.status;
          return levelOf(light(p.id, (s ?? "ready") === "ready", s === "failed" || failed(p.id)).tone);
        }
        return p.kind === "http" ? levelOf(light(p.id, true, failed(p.id)).tone) : null;
      }),
    ];
    return levels.includes("live") ? "live" : levels.includes("warn") ? "warn" : null;
  }
  function light(id: string, ready: boolean, fault: boolean): Light {
    const inUse = usedBy(config.value, id, usable).length > 0;
    // A profile that names the engine depends on it, also while it runs on the default.
    const wanted = inUse || namedBy(config.value, id).length > 0;
    if (fault || (wanted && !ready)) return isDefaultEngine(id) ? { tone: "live", title: t("engines.light.fault") } : { tone: "warn", title: t("engines.light.attention") };
    if (inUse) return { tone: "accent", title: t("engines.light.in_use") };
    if (ready) return { tone: "ok", title: t("engines.light.ready") };
    return { tone: "off", title: t("engines.light.off") };
  }
  const failed = (id: string): boolean => rowProbes[id]?.state === "bad";
  /** Who runs with the engine: the default pair's engine says so first. */
  function usersLine(id: string): string {
    const users = usedBy(config.value, id, usable);
    const names = users.map((u) => (u.isDefault ? t("engines.default_user", { name: u.name }) : u.name)).join(t("engines.list_sep"));
    if (users.length > 0) return t("engines.in_use", { names });
    const wanted = namedBy(config.value, id).map((u) => u.name).join(t("engines.list_sep"));
    return wanted ? t("engines.named_not_in_use", { names: wanted }) : t("engines.not_in_use");
  }
  const recommended = (id: string): boolean => !!manifest?.models.find((m) => m.id === id)?.recommended;

  async function test(p: ProviderConfig) {
    const of = JSON.stringify($state.snapshot(p));
    rowProbes = { ...rowProbes, [p.id]: { state: "wait", text: t("engines.testing"), of } };
    let next: { state: "ok" | "bad"; text: string };
    try {
      const r = await probeProvider($state.snapshot(p) as ProviderConfig);
      next = { state: r.ok ? "ok" : "bad", text: describeProbe(r, t) };
    } catch (e) {
      next = { state: "bad", text: `${t("engines.probe_failed")}: ${errorMessage(e)}` };
    }
    rowProbes = { ...rowProbes, [p.id]: { ...next, of } };
  }
  function openForm(p: ProviderConfig | null) {
    formInitial = p;
    formOpen = true;
  }

  // Models
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
  // The folder a file is dropped in by hand, made on the way if it is not there yet.
  async function openCustomFolder() {
    try { await openFolder(await modelsCustomDir()); } catch (e) { toast.error(errorMessage(e)); }
  }
  const loadFolder = () => modelsDirGet().then((d) => { folder = d; }).catch((e) => toast.error(errorMessage(e)));
  async function moveModels(path: string | null) {
    moving = true;
    try {
      const d = await modelsDirSet(path);
      folder = d;
      const counts = { n: d.moved.toLocaleString(), left: d.left.toLocaleString() };
      toast.ok(t(d.left ? "engines.models_dir_left" : "engines.models_dir_moved", counts));
    } catch (e) { toast.error(errorMessage(e)); }
    moving = false;
    await refreshInstalled();
  }
  async function chooseFolder() {
    const picked = await openDialog({ directory: true, multiple: false, defaultPath: folder?.path });
    if (typeof picked === "string" && picked !== folder?.path) await moveModels(picked);
  }
  const percent = (received: number, total: number): number => (total > 0 ? Math.min(100, Math.floor((received / total) * 100)) : 0);
  const entryMeta = (m: ModelEntry): string =>
    t("engines.model_meta", { size: formatBytes(entryBytes(m)), license: m.license, languages: m.languages.join(t("engines.list_sep")) });
</script>

{#snippet description()}{t("engines.description_before")}<PageLink page="flow" label={t("nav.flow")} />{t("engines.description_after")}{/snippet}
<PageHead title={t("nav.engines")} {description} />

<div class="toolbar">
  <Segmented options={roles} bind:value={() => role, (v) => guard.request(() => go("engines", v))} label={t("engines.role")} />
  <label class="search">
    <Icon name="search" />
    <input type="search" bind:value={query} placeholder={t("engines.search")} aria-label={t("engines.search")} />
  </label>
  {#if missingRecommended.length > 0}
    <button type="button" class="btn quiet" disabled={busy} onclick={downloadRecommended}><Icon name="download" />{t("engines.download_recommended")}</button>
  {/if}
</div>

{#snippet probeLine(id: string)}
  {@const probe = rowProbes[id]}
  {#if probe}<small class="probe {probe.state}" role="status">{probe.text}</small>{/if}
{/snippet}

<!-- The role's default engine is marked beside its name in the lists; any other engine that can run is made the default from its row. -->
{#snippet defaultTag(p: ProviderConfig | null)}
  {#if p && p.id === defaultId}<Tag tone="ok" title={t("engines.default_help")}>{t("engines.default_tag")}</Tag>{/if}
{/snippet}
{#snippet defaultButton(p: ProviderConfig | null)}
  {#if p && canBeDefault(p)}<button type="button" class="btn quiet" onclick={() => makeDefault(p)}>{t("engines.make_default")}</button>{/if}
{/snippet}

{#snippet localRow(r: LocalRow)}
    <Row>
      {#snippet leading()}
        {@const deaf = r.model !== null && encoderMissing(r.model)}
        {@const lamp = light(r.id, r.state === "installed" && !deaf, r.state === "error" || r.state === "gone" || deaf || failed(r.id))}
        <Ring tone={lamp.tone} title={lamp.title} />
      {/snippet}
      <!-- Words only for what the ring cannot say; a dimmed row is one nobody depends on yet. -->
      <div class="info" class:dim={r.state === "missing" && !r.engine}>
        <div class="title">
          <span class="name">{r.label}</span>
          {#if r.model?.custom}<Tag title={t("engines.tip.imported")}>{t("engines.tag.imported")}</Tag>{/if}
          {#if recommended(r.id)}<Tag tone="accent" title={t("engines.tip.recommended")}>{t("engines.recommended")}</Tag>{/if}
          {@render defaultTag(r.engine)}
          {#if r.state === "downloading"}<Tag tone="warn">{t("engines.downloading")}</Tag>
          {:else if r.state === "error"}<Tag tone={rowLevel(r) ?? "warn"} title={r.error}>{t("engines.download_failed")}</Tag>
          {:else if r.state === "gone"}<Tag tone={rowLevel(r) ?? "warn"} title={t("engines.tip.file_missing")}>{t("engines.file_missing")}</Tag>
          {:else if r.state === "missing" && r.engine}<Tag tone={rowLevel(r) ?? "warn"} title={t("engines.tip.not_downloaded")}>{t("engines.not_downloaded")}</Tag>
          {:else if r.model && encoderMissing(r.model)}<Tag tone={rowLevel(r) ?? "warn"} title={t("engines.tip.encoder_missing")}>{t("engines.encoder_missing")}</Tag>{/if}
          {#if r.engine && !hasRole(r.engine)}<Tag tone="warn">{t("engines.no_role")}</Tag>{/if}
        </div>
        {#if r.meta}<small class="line" use:tip={r.meta}>{r.meta}</small>{/if}
        {#if r.desc}<small>{r.desc}</small>{/if}
        {#if r.engine}<small>{usersLine(r.id)}</small>{/if}
        {#if r.state === "error"}<small class="probe bad">{r.error}</small>{/if}
        {@render probeLine(r.id)}
      </div>
      {#if r.state === "installed"}
        {#if !(r.model && encoderMissing(r.model))}{@render defaultButton(r.engine)}{/if}
        {#if r.engine && !(r.model && encoderMissing(r.model))}<button type="button" class="btn quiet" onclick={() => test(r.engine!)} disabled={rowProbes[r.id]?.state === "wait"}>{t("engines.test")}</button>{/if}
        <button type="button" class="btn quiet danger" onclick={() => { deleting = r.model; }}>{t("engines.delete_model")}</button>
      {:else if r.state === "downloading"}
        {@const pct = percent(r.received, r.total)}
        <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={r.label}><i style:width="{pct}%"></i></div>
        <span class="sub num pct">{pct}%</span>
        <button type="button" class="btn quiet" onclick={() => cancel(r.id)}>{t("common.cancel")}</button>
      {:else if r.state === "error"}
        <button type="button" class="btn" onclick={() => download(r.id)}>{t("engines.retry")}</button>
      {:else if r.state === "missing"}
        <button type="button" class="btn" onclick={() => download(r.id)}><Icon name="download" />{t("engines.download")}</button>
      {/if}
    </Row>
{/snippet}

<!-- Every installed model is an engine of its own; a file still to download is listed with it. -->
<Group title={t("engines.local_models")}>
  {#snippet actions()}
    <div class="pills" role="radiogroup" aria-label={t("engines.filter")}>
      {#each filters as f (f.value)}
        <button type="button" class="pill" class:on={filter === f.value} role="radio" aria-checked={filter === f.value} onclick={() => (filter = f.value)}>{f.label}<span class="num">{f.count}</span></button>
      {/each}
    </div>
    <button type="button" class="btn" onclick={refreshAll} disabled={refreshing}><Icon name="refresh" size={14} />{t("common.refresh")}</button>
  {/snippet}
  {#if !manifest || !installedKnown}
    <Row><span class="sub">{t("common.loading")}</span></Row>
  {/if}
  {#each shownRows as r (r.id)}
    {@render localRow(r)}
  {:else}
    {#if manifest && installedKnown && q}<Row><span class="sub">{t("engines.no_match")}</span></Row>
    {:else if manifest && installedKnown && filter !== "all"}<Row><span class="sub">{t(filter === "own" ? "engines.none_own" : "engines.none_installed")}</span></Row>{/if}
  {/each}
  {#if foldRows.length > 0}
    <button type="button" class="foldrow" aria-expanded={foldOpen} onclick={() => { foldChoice = !foldOpen; rememberFold(foldChoice); }}>
      <span class="chev" class:open={foldOpen}><Icon name="chevron-right" size={14} /></span>
      <span>{t("engines.downloadable")}</span><span class="sub">{t("engines.downloadable_count", { count: String(foldRows.length) })}</span>
    </button>
    {#if foldOpen}
      <Fold>{#each foldRows as r (r.id)}{@render localRow(r)}{/each}</Fold>
    {/if}
  {/if}
  <div class="rowfoot">
    {#if folder}<p>{t(folder.is_default ? "engines.folder_help_default" : "engines.folder_help", { dir: folder.path })}<InfoTip info={t("engines.folder_info")} /></p>{/if}
    <p class="links">
      <button type="button" class="link" onclick={openCustomFolder}>{t("engines.open_folder")}</button>
      <button type="button" class="link" onclick={importFile}>{t("engines.import")}</button>
      <button type="button" class="link" disabled={moving} onclick={chooseFolder}>{t("engines.models_dir_choose")}</button>
      {#if folder && !folder.is_default}<button type="button" class="link" disabled={moving} onclick={() => moveModels(null)}>{t("engines.models_dir_reset")}</button>{/if}
    </p>
  </div>
</Group>

{#if shownBuiltins.length > 0}
  <!-- The platform's own services; their engines are made by the app and carry the service's name. -->
  <Group title={t("engines.builtin")}>
    {#each shownBuiltins as p (p.id)}
      {@const s = nativeStates[p.id]}
      <Row>
        {#snippet leading()}
          {@const lamp = light(p.id, s?.status === "ready", s?.status === "failed" || failed(p.id))}
          <Ring tone={lamp.tone} title={lamp.title} />
        {/snippet}
        <div class="info">
          <div class="title">
            <span class="name">{p.label}</span>
            {@render defaultTag(p)}
            {#if s && s.status !== "ready"}<Tag tone={s.status === "failed" ? "live" : "warn"} title={s.reason ?? undefined}>{t("engines.native." + s.status)}</Tag>{/if}
          </div>
          <small>{engineDescription(p, manifest)}</small>
          <small>{usersLine(p.id)}</small>
          {@render probeLine(p.id)}
        </div>
        {#if p.native_id === NATIVE_DICTATION && s?.status === "needs_download"}
          <button type="button" class="btn" onclick={() => installLanguage(p)} disabled={installing}>
            {installing ? t("engines.installing", { percent: `${Math.round(installFraction * 100)}%` }) : t("engines.install_language")}
          </button>
        {/if}
        {@render defaultButton(p)}
        <button type="button" class="btn quiet" onclick={() => test(p)} disabled={rowProbes[p.id]?.state === "wait"}>{t("engines.test")}</button>
      </Row>
    {/each}
  </Group>
{/if}

<!-- The only engines made by hand. -->
<Group title={t("engines.external")}>
  {#snippet actions()}
    <!-- With no service yet, the empty state carries the only add button. -->
    {#if shownExternals.length > 0 || q}<button type="button" class="btn" onclick={() => openForm(null)}><Icon name="plus" size={14} />{t("engines.add")}</button>{/if}
  {/snippet}
  {#each shownExternals as p (p.id)}
    <Row>
      {#snippet leading()}
        {@const lamp = light(p.id, true, failed(p.id))}
        <Ring tone={lamp.tone} title={lamp.title} />
      {/snippet}
      <div class="info">
        <div class="title">
          <span class="name">{p.label}</span>
          {@render defaultTag(p)}
          {#if !hasRole(p)}<Tag tone="warn">{t("engines.no_role")}</Tag>{/if}
        </div>
        <small>{engineDescription(p, manifest)}</small>
        <small>{usersLine(p.id)}</small>
        {@render probeLine(p.id)}
      </div>
      {@render defaultButton(p)}
      <button type="button" class="btn quiet" onclick={() => test(p)} disabled={rowProbes[p.id]?.state === "wait"}>{t("engines.test")}</button>
      <button type="button" class="btn quiet" onclick={() => openForm(p)}>{t("common.edit")}</button>
    </Row>
  {:else}
    {#if q}<Row><span class="sub">{t("engines.no_match")}</span></Row>{:else}
    <Empty text={t("engines.none_external." + role)}>
      {#snippet action()}<button type="button" class="btn" onclick={() => openForm(null)}><Icon name="plus" size={14} />{t("engines.add")}</button>{/snippet}
    </Empty>{/if}
  {/each}
</Group>

<!-- How the engines are run: timeouts, retries, unloading; saved with its own bar. -->
<General section="processing" />

<div class="foot">
  {#if manifest}<p class="help">{t(`engines.source.${manifest.source}`)}</p>{/if}
  <p class="help">
    {t("engines.speed")}
    <button type="button" class="link" onclick={() => guard.request(() => go("speed"))}>{t("speed.title")}</button>
  </p>
  <p class="help">
    {t("engines.quality")}
    <button type="button" class="link" onclick={() => guard.request(() => go("quality"))}>{t("quality.title")}</button>
  </p>
</div>

<Modal bind:open={() => deleting !== null, (v) => { if (!v) deleting = null; }} title={t("engines.delete_model_title")}>
  {#if deleting}
    {@const named = namedBy(config.value, deleting.id).map((u) => u.name)}
    {@const defaultOf = (["asr", "llm"] as Role[]).filter((r) => config.value?.default_engines[r] === deleting!.id)}
    <p>{t("engines.delete_model_body", { name: modelLabel(manifest, deleting), size: formatBytes(deleting.size_bytes) })}</p>
    <!-- What the deletion does to dictation, before it is done. -->
    {#each defaultOf as r (r)}<p class="consequence live">{t("engines.delete_default", { role: t(`engines.role.${r}`) })}</p>{/each}
    {#if named.length > 0}<p class="consequence" class:live={defaultOf.length > 0}>{t(defaultOf.length > 0 ? "engines.delete_named_default" : "engines.delete_named", { names: named.join(t("engines.list_sep")) })}</p>{/if}
  {/if}
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (deleting = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={removeModel}>{t("engines.delete_model")}</button>
  {/snippet}
</Modal>

{#if formOpen}
  <EngineForm bind:open={formOpen} initial={formInitial} {role} />
{/if}

<style>
  .info { flex: 1; min-width: 0; }
  .info.dim .name, .info.dim small { color: var(--ink-3); }
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 12px; }
  .toolbar .search { flex: 1 1 200px; max-width: 320px; display: flex; align-items: center; gap: 9px; height: var(--control); padding: 0 12px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink-3); cursor: text; }
  .toolbar .search input { flex: 1; min-width: 0; height: 100%; padding: 0; border: 0; background: none; color: var(--ink); font-size: var(--text-control); outline: none; -webkit-appearance: none; appearance: none; }
  .toolbar .search:focus-within { border-color: var(--accent); }
  .toolbar > .btn { margin-left: auto; }
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
  /* Where a file goes by hand, at the foot of the list it then appears in. */
  .rowfoot { padding: 10px var(--row-x, 20px) 12px; border-top: 1px solid var(--line); background: var(--sunken); color: var(--ink-2); font-size: var(--text-sm); }
  .rowfoot p { margin: 0; overflow-wrap: anywhere; }
  .rowfoot .links { margin-top: 2px; }
  .rowfoot :global(.info-btn) { margin-left: 2px; }
  .rowfoot .links .link { margin: 0 14px 0 0; }
  .pills { display: flex; gap: 6px; margin-right: 6px; }
  .pill { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 12px; border-radius: 15px; border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink-2); font-size: var(--text-sm); cursor: default; }
  .pill .num { color: var(--ink-3); font-variant-numeric: tabular-nums; }
  .pill.on { border-color: transparent; background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .pill.on .num { color: inherit; }
  /* The last row of the full list: the catalogue models still to download, shown on request. */
  .foldrow { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 54px; padding: 0 var(--row-x, 20px); border: 0; border-top: 1px solid var(--line); background: none; color: var(--ink); font-size: var(--text-control); font-weight: 600; text-align: left; cursor: default; }
  .foldrow .sub { color: var(--ink-2); font-weight: 400; font-size: var(--text-sm); }
  .foldrow .chev { display: inline-flex; color: var(--ink-3); transition: transform .2s; }
  .foldrow .chev.open { transform: rotate(90deg); }
  .consequence { color: var(--warn); }
  .consequence.live { color: var(--live); }
  .foot { margin-top: 8px; }
  .foot .help { margin-top: 4px; }
  .link { border: 0; background: none; padding: 0; margin-left: 6px; color: var(--accent); font-size: var(--text-sm); cursor: default; }
  .link:hover { color: var(--ink); }
  .link:disabled { opacity: .5; }
</style>
