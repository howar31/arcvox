<script lang="ts">
  import { onMount } from "svelte";
  import {
    errorMessage, listHistory, listMicrophones, modelsInstalled, openPrivacyPane, permissions, upsertProfile, widgetCopy,
    type Profile, type ProviderConfig, type Utterance,
  } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { formatDateTime, formatShortDate, formatTime } from "../../lib/format";
  import { t } from "../../lib/i18n";
  import { defaultProfile, profileName } from "../../lib/labels";
  import { appsSummary, engineMeta, NEW_PROFILE, withLlm } from "../../lib/profileEdit";
  import { go } from "../../lib/route";
  import { client, config } from "../../lib/store.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  type Role = "asr" | "llm";

  let access = $state<boolean | null>(null);
  let mics = $state<string[]>([]);
  let installedIds = $state<Set<string> | null>(null);
  let installedPaths = $state<Set<string>>(new Set());
  let recent = $state<Utterance[] | null>(null);

  async function checkAccess() {
    try { access = (await permissions()).accessibility; } catch { access = null; }
  }

  async function loadRecent() {
    try { recent = await listHistory(3); } catch (e) { recent ??= []; toast.error(errorMessage(e)); }
  }

  onMount(() => {
    checkAccess();
    listMicrophones().then((m) => { mics = m; }).catch(() => { mics = []; });
    modelsInstalled().then((m) => { installedIds = new Set(m.map((x) => x.id)); installedPaths = new Set(m.map((x) => x.path)); }).catch(() => { installedIds = null; });
    loadRecent();
    const onFocus = () => { checkAccess(); loadRecent(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  const profile = $derived(defaultProfile(config.value));
  const providers = $derived(config.value?.providers ?? []);
  const others = $derived((config.value?.profiles ?? []).filter((p) => !p.is_default));
  const choices = $derived({
    asr: providers.filter((p) => p.capabilities.transcribe),
    llm: providers.filter((p) => p.capabilities.chat),
  });

  $effect(() => { ensureAppNames(others.flatMap((p) => p.matcher.app_ids)); });

  const find = (id: string | null): ProviderConfig | null => providers.find((p) => p.id === id) ?? null;

  // Capture stage
  const mic = $derived(client.value?.microphone ?? "builtin");
  const micName = $derived(mic === "builtin" ? t("home.mic.builtin_name") : mic === "default" ? t("general.mic_default") : mic);
  const micAbsent = $derived(mic !== "builtin" && mic !== "default" && !mics.includes(mic));
  const micMeta = $derived(
    mic === "builtin" ? t("home.mic.builtin") : mic === "default" ? t("home.mic.default") : micAbsent ? t("home.mic.absent") : t("home.mic.device"),
  );

  function engineLine(id: string | null): { name: string; known: boolean; meta: string; missing: boolean } {
    if (id === null) return { name: t("home.no_rewrite"), known: true, meta: t("home.no_rewrite_meta"), missing: false };
    const p = find(id);
    if (!p) return { name: id, known: false, meta: "", missing: false };
    const m = engineMeta(p, installedIds ?? new Set(), installedIds ? installedPaths : undefined);
    if (m.kind === "builtin") return { name: p.label, known: true, meta: t("home.meta.builtin"), missing: false };
    if (m.kind === "external") return { name: p.label, known: true, meta: m.detail ?? t("home.meta.external"), missing: false };
    if (installedIds === null) return { name: p.label, known: true, meta: t("home.meta.local"), missing: false };
    return { name: p.label, known: true, meta: t(m.ready ? "home.meta.local_ready" : "home.meta.local_missing"), missing: !m.ready };
  }

  const hasOpencc = $derived(!!profile?.steps.some((s) => s.step === "opencc"));

  // The selects are one-way bound: after a failed save the element is put back on the stored value,
  // so the menu shows what is in use and choosing the same option again fires a change.
  async function changeEngine(role: Role, select: HTMLSelectElement) {
    if (!profile) return;
    const value = select.value;
    const next = role === "asr" ? { ...profile, asr_provider: value } : withLlm(profile, value === "" ? null : value);
    try {
      await upsertProfile($state.snapshot(next) as Profile);
      toast.ok(t("common.saved"));
    } catch (e) {
      const current = defaultProfile(config.value);
      select.value = (role === "asr" ? current?.asr_provider : current?.llm_provider) ?? "";
      toast.error(errorMessage(e));
    }
  }
  async function changeMic(select: HTMLSelectElement) {
    if (!client.value) return;
    try {
      await client.save({ ...$state.snapshot(client.value), microphone: select.value });
      toast.ok(t("common.saved"));
    } catch (e) {
      select.value = client.value?.microphone ?? "builtin";
      toast.error(errorMessage(e));
    }
  }

  function appsLine(p: Profile): string {
    const names = [...p.matcher.app_ids.map(appName), ...p.matcher.url_patterns];
    if (names.length === 0) return t("home.no_apps");
    const { shown, more } = appsSummary(names);
    const list = shown.join(t("home.list_sep"));
    return more > 0 ? t("home.apps_more", { names: list, count: String(more) }) : list;
  }

  async function copy(text: string) {
    try { await widgetCopy(text); toast.ok(t("home.copied")); } catch (e) { toast.error(errorMessage(e)); }
  }

  function when(ts: number): { short: string; full: string } {
    const today = new Date().toDateString() === new Date(ts * 1000).toDateString();
    return { short: today ? formatTime(ts) : formatShortDate(ts), full: formatDateTime(ts) };
  }
</script>

{#snippet engineName(id: string | null)}
  <span class="eng">
    {#if id !== null && !find(id)}<span class="line unknown">{id}</span><Tag tone="warn">{t("home.not_found")}</Tag>
    {:else}<span class="line">{id === null ? t("home.no_rewrite") : find(id)?.label}</span>{/if}
  </span>
{/snippet}

<PageHead title={t("nav.home")} description={t("home.description")} />

{#if access === false}
  <div class="attention" role="alert">
    <Ring tone="live" />
    <span class="grow">{t("home.access_needed")}</span>
    <button type="button" class="btn" onclick={() => openPrivacyPane("accessibility").catch((e) => toast.error(errorMessage(e)))}>{t("general.open_system_settings")}</button>
  </div>
{/if}

{#if profile}
  {@const asr = engineLine(profile.asr_provider)}
  {@const llm = engineLine(profile.llm_provider)}
  <div class="bar">
    <h2>{t("home.flow")}</h2>
    <Tag tone="accent"><Ring tone="accent" size="sm" />{t("home.profile_tag", { name: profile.name })}</Tag>
  </div>
  <div class="flow"><div class="path">
    <div class="stage">
      <div class="role">{t("home.stage.capture")}</div>
      <div class="what" title={micName}>{micName}</div>
      <div class="meta" class:warn={micAbsent}>{micMeta}</div>
      <span class="menu act">
        <span class="btn quiet" aria-hidden="true">{t("home.change")}</span>
        <select value={mic} aria-label={t("home.change_mic")} onchange={(e) => changeMic(e.currentTarget)}>
          <option value="builtin">{t("general.mic_builtin")}</option>
          <option value="default">{t("general.mic_default")}</option>
          {#each mics as m (m)}<option value={m}>{m}</option>{/each}
          {#if micAbsent}<option value={mic} disabled>{mic}</option>{/if}
        </select>
      </span>
    </div>
    {#each [["asr", profile.asr_provider, asr], ["llm", profile.llm_provider, llm]] as const as [role, id, line] (role)}
      <div class="stage engine">
        <div class="role">{t(`home.stage.${role}`)}</div>
        <div class="what" title={line.name}>
          {#if line.known}{line.name}{:else}<span class="unknown">{line.name}</span>{/if}
        </div>
        <div class="meta" class:warn={!line.known || line.missing}>
          {#if !line.known}<Tag tone="warn">{t("home.not_found")}</Tag>{:else}{line.meta}{/if}
        </div>
        {#if line.missing}
          <button type="button" class="btn quiet act" onclick={() => go("engines", role)}>{t("home.download")}</button>
        {:else}
          <span class="menu act">
            <span class="btn quiet" aria-hidden="true">{t("home.change")}</span>
            <select value={id ?? ""} aria-label={t(`home.change_${role}`)} onchange={(e) => changeEngine(role, e.currentTarget)}>
              {#if role === "llm"}<option value="">{t("home.no_rewrite")}</option>{/if}
              {#each choices[role] as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
              {#if id !== null && !choices[role].some((p) => p.id === id)}<option value={id} disabled>{id}</option>{/if}
            </select>
          </span>
        {/if}
      </div>
    {/each}
    <div class="stage">
      <div class="role">{t("home.stage.output")}</div>
      <div class="what">{t("home.output_paste")}</div>
      <div class="meta">{t(hasOpencc ? "home.output_opencc" : "home.output_plain")}</div>
      <button type="button" class="btn quiet act" onclick={() => go("profiles", profile.id)}>{t("home.adjust")}</button>
    </div>
  </div>
  </div>
  <p class="help">{t("home.flow_help", { name: profile.name })}</p>

  <Group title={t("home.by_app")}>
    {#each others as p (p.id)}
      <Row onclick={() => go("profiles", p.id)}>
        <span class="who">
          <span class="name line">{p.name}</span>
          <span class="sub line">{appsLine(p)}</span>
        </span>
        <span class="pair">{@render engineName(p.asr_provider)}<span class="arrow" aria-hidden="true">→</span>{@render engineName(p.llm_provider)}</span>
      </Row>
    {:else}
      <Empty text={t("home.by_app_empty")}>
        {#snippet action()}<button type="button" class="btn" onclick={() => go("profiles", NEW_PROFILE)}>{t("home.add_profile")}</button>{/snippet}
      </Empty>
    {/each}
  </Group>
{/if}

{#if recent !== null && config.value}
  <div class="bar">
    <h2>{t("home.recent")}</h2>
    {#if recent.length}<button type="button" class="btn quiet link" onclick={() => go("history")}>{t("home.see_all")}</button>{/if}
  </div>
  <div class="titled">
    <Group>
      {#each recent as u (u.id)}
        {@const time = when(u.ts)}
        <Row>
          <span class="time" title={time.full}>{time.short}</span>
          <span class="who">
            <span class="final">{u.final_text}</span>
            <span class="sub line">{[u.app_name, profileName(config.value, u.profile_id)].filter(Boolean).join(t("home.meta_sep"))}</span>
          </span>
          <button type="button" class="btn quiet icon-btn" aria-label={t("widget.copy")} title={t("widget.copy")} onclick={() => copy(u.final_text)}><Icon name="copy" /></button>
        </Row>
      {:else}
        <Empty text={t("home.recent_empty")} />
      {/each}
    </Group>
  </div>
{/if}

<style>
  .attention { display: flex; align-items: center; gap: 10px; margin-bottom: 4px; padding: 10px 14px; border-radius: 10px; background: color-mix(in srgb, var(--live) 10%, var(--surface)); border: 1px solid color-mix(in srgb, var(--live) 35%, transparent); }
  .grow { flex: 1; min-width: 0; }

  .bar { display: flex; align-items: center; gap: 10px; min-height: 20px; margin: 22px 2px 8px; }
  .bar h2 { flex: 1; min-width: 0; margin: 0; font: 600 12px/1 var(--font); color: var(--ink-2); }
  .bar .link { height: 20px; font-size: 12px; margin-right: -6px; }

  /* The processing flow: one strip, one stage per step; a column when narrow. */
  .flow { container: flow / inline-size; }
  .path { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); background: var(--surface); border: 1px solid var(--line); border-radius: 12px; overflow: hidden; }
  .stage { padding: 14px 14px 13px; position: relative; min-width: 0; display: flex; flex-direction: column; align-items: flex-start; }
  .stage + .stage { border-left: 1px solid var(--line); }
  .stage + .stage::before { content: ""; position: absolute; left: -5px; top: 21px; width: 8px; height: 8px; border-top: 1.5px solid var(--ink-3); border-right: 1.5px solid var(--ink-3); transform: rotate(45deg); background: var(--surface); }
  .stage.engine { background: linear-gradient(var(--accent-soft), var(--accent-soft)); }
  .role { font-size: 11px; color: var(--ink-2); }
  .what { font: 600 14px/1.3 var(--font-round); margin: 8px 0 3px; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: 12px; color: var(--ink-2); max-width: 100%; flex: 1; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .meta.warn, .unknown { color: var(--warn); }
  .act { margin: 8px 0 0 -6px; }
  @container flow (width < 520px) {
    .path { grid-template-columns: minmax(0, 1fr); }
    .stage { display: grid; grid-template-columns: 104px minmax(0, 1fr) auto; grid-template-areas: "role what act" "role meta act"; column-gap: 12px; align-items: center; padding: 9px 14px; }
    .stage + .stage { border-left: 0; border-top: 1px solid var(--line); }
    .stage + .stage::before { left: 22px; top: -5px; transform: rotate(135deg); }
    .role { grid-area: role; }
    .what { grid-area: what; margin: 0; }
    .meta { grid-area: meta; -webkit-line-clamp: 1; line-clamp: 1; }
    .act { grid-area: act; margin: 0 -6px 0 0; }
  }

  /* A native menu: the select is invisible and covers the quiet button. */
  .menu { position: relative; display: inline-flex; }
  .menu select { position: absolute; inset: 0; width: 100%; height: 100%; opacity: 0; padding: 0; border: 0; cursor: default; }
  .menu:hover .btn { background: var(--accent-soft); }
  .menu:has(select:focus-visible) .btn { outline: 2px solid var(--accent); outline-offset: 1px; }

  .who { display: flex; flex-direction: column; flex: 1; min-width: 0; }
  .line { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* The two engine names share the space; each truncates on its own. */
  .pair { display: flex; align-items: center; gap: 5px; flex: 0 1 auto; min-width: 0; max-width: 52%; color: var(--ink-2); }
  .eng { display: flex; align-items: center; gap: 5px; flex: 0 1 auto; min-width: 0; max-width: 50%; }
  .arrow { flex: none; color: var(--ink-3); }
  .final { line-height: 1.55; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .icon-btn { width: 26px; padding: 0; margin-right: -6px; align-self: flex-start; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
  .time { width: 44px; flex: none; align-self: flex-start; padding-top: 1px; color: var(--ink-2); font-size: 12px; font-variant-numeric: tabular-nums; }
  .titled :global(.section.untitled) { margin-top: 0; }
</style>
