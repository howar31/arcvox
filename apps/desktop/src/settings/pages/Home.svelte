<script lang="ts">
  import { onMount } from "svelte";
  import {
    errorMessage, historyStats, listHistory, listMicrophones, modelsInstalled, openPrivacyPane, permissions, upsertProfile, widgetCopy,
    type HistoryStats, type Profile, type ProviderConfig, type Utterance,
  } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { formatDateTime, formatShortDate, formatTime, uiLocale } from "../../lib/format";
  import { averageSeconds } from "../../lib/historyView";
  import { t } from "../../lib/i18n";
  import { defaultProfile, profileName } from "../../lib/labels";
  import { appsSummary, engineMeta, withLlm } from "../../lib/profileEdit";
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
  import ProfileForm from "./ProfileForm.svelte";

  type Role = "asr" | "llm";

  let access = $state<boolean | null>(null);
  let mics = $state<string[]>([]);
  let installedIds = $state<Set<string> | null>(null);
  let installedPaths = $state<Set<string>>(new Set());
  let recent = $state<Utterance[] | null>(null);
  // Totals of today and of the last seven days, today included.
  let stats = $state<{ today: HistoryStats; week: HistoryStats } | null>(null);
  // The profile open in the editor: its id, or NEW_PROFILE.
  let editing = $state<string | null>(null);

  async function checkAccess() {
    try { access = (await permissions()).accessibility; } catch { access = null; }
  }

  async function loadRecent() {
    try { recent = await listHistory(3); } catch (e) { recent ??= []; toast.error(errorMessage(e)); }
  }

  async function loadStats() {
    const midnight = new Date();
    midnight.setHours(0, 0, 0, 0);
    const today = Math.floor(midnight.getTime() / 1000);
    midnight.setDate(midnight.getDate() - 6);
    const week = Math.floor(midnight.getTime() / 1000);
    try {
      const [a, b] = await Promise.all([historyStats(today), historyStats(week)]);
      stats = { today: a, week: b };
    } catch {
      // The totals are left out; the list below reports a history that cannot be read.
      stats = null;
    }
  }

  onMount(() => {
    checkAccess();
    listMicrophones().then((m) => { mics = m; }).catch(() => { mics = []; });
    modelsInstalled().then((m) => { installedIds = new Set(m.map((x) => x.id)); installedPaths = new Set(m.map((x) => x.path)); }).catch(() => { installedIds = null; });
    loadRecent();
    loadStats();
    const onFocus = () => { checkAccess(); loadRecent(); loadStats(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });

  // The profile whose flow is shown: the default one until another is picked.
  let shownId = $state<string | null>(null);
  // The default profile first, the others in their stored order.
  const profiles = $derived([...(config.value?.profiles ?? [])].sort((a, b) => Number(b.is_default) - Number(a.is_default)));
  const profile = $derived(profiles.find((p) => p.id === shownId) ?? defaultProfile(config.value));
  const providers = $derived(config.value?.providers ?? []);
  const choices = $derived({
    asr: providers.filter((p) => p.capabilities.transcribe),
    llm: providers.filter((p) => p.capabilities.chat),
  });

  $effect(() => { if (profile) ensureAppNames(profile.matcher.app_ids); });

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
      const current = config.value?.profiles.find((p) => p.id === profile.id);
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

  /** The first applications of a profile as one phrase; null when it has none. */
  function appsList(p: Profile): string | null {
    const names = [...p.matcher.app_ids.map(appName), ...p.matcher.url_patterns];
    if (names.length === 0) return null;
    const { shown, more } = appsSummary(names);
    const list = shown.join(t("home.list_sep"));
    return more > 0 ? t("home.apps_more", { names: list, count: String(more) }) : list;
  }
  function flowHelp(p: Profile): string {
    if (p.is_default) return t("home.flow_help", { name: p.name });
    const apps = appsList(p);
    return apps === null ? t("home.flow_help_unused") : t("home.flow_help_other", { apps });
  }

  async function copy(text: string) {
    try { await widgetCopy(text); toast.ok(t("home.copied")); } catch (e) { toast.error(errorMessage(e)); }
  }

  function when(ts: number): { short: string; full: string } {
    const today = new Date().toDateString() === new Date(ts * 1000).toDateString();
    return { short: today ? formatTime(ts) : formatShortDate(ts), full: formatDateTime(ts) };
  }
</script>

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
    <h2>{profile.is_default ? t("home.flow_default") : t("home.flow_of", { name: profile.name })}</h2>
    {#if profiles.length > 1}
      <span class="menu pick">
        <Tag tone="accent"><Ring tone="accent" size="sm" /><span class="picked">{profile.name}</span><Icon name="chevron-down" size={12} /></Tag>
        <select value={profile.id} aria-label={t("home.pick_profile")} onchange={(e) => (shownId = e.currentTarget.value)}>
          {#each profiles as p (p.id)}<option value={p.id}>{p.is_default ? t("home.default_option", { name: p.name }) : p.name}</option>{/each}
        </select>
      </span>
    {:else}
      <Tag tone="accent"><Ring tone="accent" size="sm" />{t("home.profile_tag", { name: profile.name })}</Tag>
    {/if}
  </div>
  <div class="path">
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
      <button type="button" class="btn quiet act" onclick={() => (editing = profile.id)}>{t("home.adjust")}</button>
    </div>
  </div>
  <p class="help">{flowHelp(profile)}</p>
{/if}

{#if stats}
  {@const number = (n: number) => n.toLocaleString(uiLocale())}
  {@const average = (s: HistoryStats) => { const v = averageSeconds(s); return v === null ? t("home.stat.none") : t("home.stat.secs", { secs: v }); }}
  <div class="bar"><h2>{t("home.today")}</h2></div>
  <div class="stats">
    {#each [
      [t("home.stat.count"), number(stats.today.count), number(stats.week.count)],
      [t("home.stat.chars"), number(stats.today.chars), number(stats.week.chars)],
      [t("home.stat.average"), average(stats.today), average(stats.week)],
    ] as [label, value, week] (label)}
      <div class="stat">
        <div class="role">{label}</div>
        <div class="value num">{value}</div>
        <div class="meta">{t("home.stat.week", { value: week })}</div>
      </div>
    {/each}
  </div>
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

{#if editing !== null}
  {#key editing}<ProfileForm arg={editing} bind:open={() => true, (v) => { if (!v) editing = null; }} />{/key}
{/if}

<style>
  .attention { display: flex; align-items: center; gap: 10px; margin-bottom: 4px; padding: 12px var(--row-x); border-radius: var(--radius-group); background: color-mix(in srgb, var(--live) 10%, var(--surface)); border: 1px solid color-mix(in srgb, var(--live) 35%, transparent); }
  .grow { flex: 1; min-width: 0; }

  .bar { display: flex; align-items: center; gap: 10px; min-height: 22px; margin: 26px 4px 10px; }
  .bar h2 { flex: 1; min-width: 0; margin: 0; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); }
  .bar .link { height: 22px; font-size: var(--text-sm); margin-right: -8px; }

  /* The processing flow: one strip, one stage per step. The window's minimum width keeps the four stages side by side. */
  .path { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); overflow: hidden; }
  .stage { padding: 16px var(--row-x) 14px; position: relative; min-width: 0; display: flex; flex-direction: column; align-items: flex-start; }
  .stage + .stage { border-left: 1px solid var(--line); }
  .stage + .stage::before { content: ""; position: absolute; left: -5px; top: 24px; width: 8px; height: 8px; border-top: 1.5px solid var(--ink-3); border-right: 1.5px solid var(--ink-3); transform: rotate(45deg); background: var(--surface); }
  .stage.engine { background: linear-gradient(var(--accent-soft), var(--accent-soft)); }
  .role { font-size: var(--text-xs); color: var(--ink-2); }
  .what { font: 600 var(--text-lg)/1.3 var(--font-round); margin: 10px 0 4px; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: var(--text-sm); color: var(--ink-2); max-width: 100%; flex: 1; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .meta.warn, .unknown { color: var(--warn); }
  .act { margin: 10px 0 0 -8px; }

  /* A native menu: the select is invisible and covers the quiet button. */
  .menu { position: relative; display: inline-flex; }
  .menu select { position: absolute; inset: 0; width: 100%; height: 100%; opacity: 0; padding: 0; border: 0; cursor: default; }
  .menu:hover .btn { background: var(--accent-soft); }
  /* The profile picker: the tag is the button. */
  .pick { min-width: 0; max-width: 50%; }
  .pick :global(.tag) { min-width: 0; max-width: 100%; }
  .picked { overflow: hidden; text-overflow: ellipsis; }
  .pick:has(select:focus-visible) :global(.tag) { outline: 2px solid var(--accent); outline-offset: 1px; }
  .menu:has(select:focus-visible) .btn { outline: 2px solid var(--accent); outline-offset: 1px; }

  .who { display: flex; flex-direction: column; flex: 1; min-width: 0; }
  .line { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* Today's totals: one strip, like the flow above it. */
  .stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); overflow: hidden; }
  .stat { padding: 14px var(--row-x) 13px; min-width: 0; }
  .stat + .stat { border-left: 1px solid var(--line); }
  .value { font: 600 22px/1.25 var(--font-round); margin: 6px 0 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .final { line-height: 1.55; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .icon-btn { width: var(--control); padding: 0; margin-right: -8px; align-self: flex-start; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
  .time { width: 50px; flex: none; align-self: flex-start; padding-top: 2px; color: var(--ink-2); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }
  .titled :global(.section.untitled) { margin-top: 0; }
</style>
