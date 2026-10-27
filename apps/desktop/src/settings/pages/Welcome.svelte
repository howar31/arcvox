<script lang="ts">
  import { onMount } from "svelte";
  import { enable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { errorMessage, formatBytes, historyLast, modelsInstalled, modelsManifest, nativeStatus, onDictationState, onHistoryChanged, openPrivacyPane, permissions, requestAccessibility, requestMicrophone, systemInfo, type ClientSettings, type InstalledModel, type ManifestView, type ModelEntry, type NativeStatusKind, type Permissions, type Utterance, type WidgetState } from "../../lib/api";
  import type { Role } from "../../lib/engines";
  import { guide } from "../../lib/guide.svelte";
  import { applePath, downloadTotals, engineStepDone, guideEntries, memoryShort, permissionStepDone } from "../../lib/guideView";
  import { t } from "../../lib/i18n";
  import { defaultProfile, modelDescription } from "../../lib/labels";
  import { go } from "../../lib/route";
  import { shortcutLabel } from "../../lib/shortcut";
  import { client, config } from "../../lib/store.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Mark from "../../lib/ui/Mark.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import LanguagePicker from "./welcome/LanguagePicker.svelte";

  const STEPS = [1, 2, 3] as const;
  const STEP_KEYS: Record<number, string> = { 1: "welcome.step.engines", 2: "welcome.step.permissions", 3: "welcome.step.try" };

  let manifest = $state<ManifestView | null>(null);
  let installed = $state<InstalledModel[]>([]);
  let memoryBytes = $state(0);
  let fm = $state<NativeStatusKind | null>(null);
  let showAll = $state(false);
  let busy = $state(false);
  let perms = $state<Permissions | null>(null);
  let dictation = $state<WidgetState>("idle");
  let last = $state<Utterance | null>(null);

  const apple = $derived(applePath(fm));
  const entriesOf = (role: Role): ModelEntry[] => guideEntries(manifest, role, showAll);
  const chosen = $derived((["asr", "llm"] as Role[]).map((role) => manifest?.models.find((m) => m.id === guide.picked[role])).filter((m): m is ModelEntry => !!m));
  const totals = $derived(downloadTotals(chosen, installed, guide.progress));
  const remaining = $derived(totals.total - totals.received);
  const installedIds = $derived(new Set(installed.map((m) => m.id)));
  const downloading = $derived(guide.started && remaining > 0);
  const canContinue = $derived(
    guide.step === 1 ? engineStepDone(guide.choice, apple, guide.started, remaining) && !busy
    : guide.step === 2 ? permissionStepDone(perms)
    : true,
  );
  const shortcuts = $derived((client.value?.hotkey.shortcuts ?? []).map(shortcutLabel));
  const hasFn = $derived((client.value?.hotkey.shortcuts ?? []).some((s) => s.includes("Fn")));
  const noEngine = $derived(guide.choice === "later");
  const listening = $derived(dictation === "recording" || dictation === "locked");
  const busyRing = $derived(dictation === "processing" || dictation === "inserting");

  // The language can be changed before anything is downloaded: the change
  // reloads the window, which starts the guide over.
  async function setLanguage(value: ClientSettings["ui_language"]) {
    const current = client.value;
    if (!current || current.ui_language === value) return;
    try {
      await client.save({ ...$state.snapshot(current), ui_language: value });
      location.reload();
    } catch (e) { toast.error(errorMessage(e)); }
  }

  function isInstalled(id: string) { return installedIds.has(id); }
  function progressOf(id: string) { return guide.progress[id]; }
  function pick(role: Role, id: string) {
    if (guide.started) return;
    guide.choice = "models";
    guide.picked = { ...guide.picked, [role]: id };
  }
  function choose(choice: "apple" | "later") {
    if (guide.started) return;
    guide.choice = choice;
  }

  async function refreshInstalled() {
    try { installed = await modelsInstalled(); } catch { /* the list stays as it was */ }
  }
  async function checkApple() {
    try { fm = (await nativeStatus("apple.foundation_models", null)).status; } catch { fm = null; }
  }
  async function checkAccess() {
    try { perms = await permissions(); } catch { perms = null; }
  }

  function startDownload() {
    const wanted = (["asr", "llm"] as Role[]).map((role) => ({ role, id: guide.picked[role] }));
    guide.download(wanted, installedIds, () => config.value).then(refreshInstalled);
  }

  async function next() {
    if (guide.step === 1 && guide.choice === "apple") {
      busy = true;
      try { await guide.useApple(config.value); } catch (e) { toast.error(errorMessage(e)); return; } finally { busy = false; }
    }
    if (guide.step < 3) guide.step = (guide.step + 1) as 2 | 3;
  }
  function back() {
    if (guide.step > 1) guide.step = (guide.step - 1) as 1 | 2;
  }

  async function finish() {
    if (guide.launchAtLogin) {
      try { if (!(await isEnabled())) await enable(); } catch (e) { toast.error(errorMessage(e)); }
    }
    try {
      if (client.value) await client.save({ ...client.value, guide_done: true });
    } catch (e) {
      toast.error(errorMessage(e));
      return;
    }
    go(noEngine ? "engines" : "home");
  }

  async function askAccessibility() {
    try { await requestAccessibility(); await checkAccess(); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function askMicrophone() {
    try { await requestMicrophone(); await checkAccess(); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function openPane(pane: "accessibility" | "microphone" | "intelligence") {
    try { await openPrivacyPane(pane); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function loadLast() {
    const profile = defaultProfile(config.value);
    if (!profile) return;
    try {
      const run = await historyLast(profile.id);
      if (run && run.row.ts >= guide.openedAt && !run.row.error) last = run.row;
    } catch { /* the trial box keeps what it shows */ }
  }

  const secs = (ms: number) => (ms / 1000).toFixed(1);

  onMount(() => {
    guide.watch();
    modelsManifest().then((m) => { manifest = m; }).catch(() => {});
    refreshInstalled();
    systemInfo().then((i) => { memoryBytes = i.memory_bytes; }).catch(() => {});
    checkApple();
    checkAccess();
    const onFocus = () => { checkAccess(); if (apple !== "ready") checkApple(); };
    window.addEventListener("focus", onFocus);
    // A download finishing shows as installed; the system switching Apple Intelligence on shows as ready;
    // a permission granted in System Settings shows as granted.
    const tick = setInterval(() => {
      if (guide.step === 1 && apple !== "none" && apple !== "ready") checkApple();
      if (guide.step === 2) checkAccess();
    }, 1500);
    const changed = onHistoryChanged(() => { loadLast(); });
    const state = onDictationState((s) => { dictation = s; });
    return () => {
      window.removeEventListener("focus", onFocus);
      clearInterval(tick);
      changed.then((off) => off());
      state.then((off) => off());
    };
  });

  $effect(() => {
    // Every chosen model arrived: the list of installed models follows.
    const done = Object.values(guide.progress).filter((p) => p.done).length;
    if (done) refreshInstalled();
  });
</script>

{#snippet modelCard(entry: ModelEntry, role: Role)}
  {@const on = guide.choice === "models" && guide.picked[role] === entry.id}
  {@const p = progressOf(entry.id)}
  <button type="button" class="pick" class:on class:dim={guide.choice !== "models" || (guide.started && !on)} role="radio" aria-checked={on} disabled={guide.started} onclick={() => pick(role, entry.id)}>
    <span class="dot"></span>
    <span class="body">
      <span class="name">{entry.label}
        {#if isInstalled(entry.id)}<Tag tone="ok">{t("welcome.installed")}</Tag>
        {:else if memoryShort(entry, memoryBytes)}<Tag tone="warn">{t("welcome.memory_short", { gb: String(entry.min_ram_gb) })}</Tag>{/if}
      </span>
      <small><span class="size">{formatBytes(entry.size_bytes + (entry.mmproj?.size_bytes ?? 0))}</span>{modelDescription(entry)}</small>
      {#if on && guide.started && !isInstalled(entry.id)}
        <span class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={p && p.total ? Math.round((p.received / p.total) * 100) : 0} aria-label={entry.label}><i style:width="{p && p.total ? (p.received / p.total) * 100 : 0}%"></i></span>
        <span class="pst">{#if p && !p.done}{t("welcome.downloading", { done: formatBytes(p.received), total: formatBytes(p.total) })}{:else}{t("welcome.waiting")}{/if}</span>
      {/if}
    </span>
  </button>
{/snippet}

<div class="guide">
  <div class="titlebar" data-tauri-drag-region></div>
  <header>
    <div class="brand"><Mark size={26} /><span><em>A</em>rcvox</span></div>
    <ol class="steps" aria-label={t("welcome.steps")}>
      {#each STEPS as s (s)}
        {#if s > 1}<li class="dash" aria-hidden="true"></li>{/if}
        <li class:on={guide.step === s} class:done={guide.step > s} aria-current={guide.step === s ? "step" : undefined}>
          <i>{#if guide.step > s}<Icon name="check" size={12} />{:else}{s}{/if}</i>{t(STEP_KEYS[s])}
        </li>
      {/each}
    </ol>
  </header>

  <main>
    <div class="pg">
    {#if guide.step === 1}
      <h1>{t("welcome.engines.title")}</h1>
      <p class="sub">{t("welcome.engines.desc")}</p>

      {#each ["asr", "llm"] as Role[] as role (role)}
        <h2>{t(`welcome.${role}`)}<span class="sp"></span><small>{t("welcome.pick_one")}</small></h2>
        <div class="cards" role="radiogroup" aria-label={t(`welcome.${role}`)}>
          {#each entriesOf(role) as entry (entry.id)}{@render modelCard(entry, role)}{/each}
        </div>
      {/each}
      <div class="more"><button type="button" class="btn quiet" onclick={() => (showAll = !showAll)}>{t(showAll ? "welcome.show_fewer" : "welcome.show_all")}</button></div>

      {#if apple !== "none"}
        <button type="button" class="pick wide" class:on={guide.choice === "apple"} class:dim={guide.started} role="radio" aria-checked={guide.choice === "apple"} disabled={guide.started} onclick={() => choose("apple")}>
          <span class="dot"></span>
          <span class="body">
            <span class="name">{t("welcome.apple.title")}
              {#if apple === "ready"}<Tag tone="ok">{t("welcome.apple.ready")}</Tag>{:else if apple === "off"}<Tag tone="warn">{t("welcome.apple.off")}</Tag>{:else}<Tag tone="warn">{t("welcome.apple.preparing")}</Tag>{/if}
            </span>
            <small>{t("welcome.apple.desc")}</small>
          </span>
          {#if apple === "off"}<span class="btn" role="button" tabindex="0" onclick={(e) => { e.stopPropagation(); openPane("intelligence"); }} onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.stopPropagation(); openPane("intelligence"); } }}>{t("general.open_system_settings")}</span>{/if}
        </button>
      {/if}

      <button type="button" class="pick wide" class:on={guide.choice === "later"} class:dim={guide.started} role="radio" aria-checked={guide.choice === "later"} disabled={guide.started} onclick={() => choose("later")}>
        <span class="dot"></span>
        <span class="body"><span class="name">{t("welcome.later")}</span><small>{t("welcome.later_note")}</small></span>
      </button>

      {#if guide.choice === "models"}
        <div class="act">
          <span class="status">
            {#if guide.error}<span class="err">{guide.error}</span>
            {:else if remaining === 0}{t("welcome.all_installed")}
            {:else if downloading}{t("welcome.downloading", { done: formatBytes(totals.received), total: formatBytes(totals.total) })}
            {:else}{t("welcome.total", { size: formatBytes(remaining) })}{/if}
          </span>
          {#if downloading}<button type="button" class="btn" onclick={() => guide.cancel()}>{t("common.cancel")}</button>
          {:else if remaining > 0}<button type="button" class="btn primary" onclick={startDownload}><Icon name="download" />{t("engines.download")}</button>{/if}
        </div>
      {/if}
    {:else if guide.step === 2}
      <h1>{t("welcome.permissions.title")}</h1>
      <p class="sub">{t("welcome.permissions.desc")}</p>
      <div class="group">
        <Row label={t("general.accessibility")} help={perms?.accessibility === false ? t("general.accessibility_missing") : t("general.accessibility_help")}>
          {#snippet tag()}{#if perms?.accessibility === false}<Tag tone="live">{t("general.permission_missing")}</Tag>{/if}{/snippet}
          {#snippet leading()}<Ring tone={perms === null ? "off" : perms.accessibility ? "ok" : "live"} />{/snippet}
          {#if perms?.accessibility === false}
            <button type="button" class="btn" onclick={askAccessibility}>{t("general.request")}</button>
            <button type="button" class="btn quiet" onclick={() => openPane("accessibility")}>{t("general.open_system_settings")}</button>
          {/if}
        </Row>
        <Row label={t("general.microphone_permission")} help={t(perms?.microphone === "denied" ? "general.microphone_denied" : perms?.microphone === "not_asked" ? "general.microphone_not_asked" : perms?.microphone === "restricted" ? "general.microphone_restricted" : "general.microphone_help")}>
          {#snippet tag()}
            {#if perms?.microphone === "denied"}<Tag tone="live">{t("general.permission_missing")}</Tag>
            {:else if perms?.microphone === "not_asked"}<Tag tone="warn">{t("general.permission_not_asked")}</Tag>
            {:else if perms?.microphone === "restricted"}<Tag tone="live">{t("general.permission_restricted")}</Tag>{/if}
          {/snippet}
          {#snippet leading()}<Ring tone={perms === null ? "off" : perms.microphone === "granted" ? "ok" : perms.microphone === "not_asked" ? "warn" : "live"} />{/snippet}
          {#if perms?.microphone === "not_asked"}<button type="button" class="btn" onclick={askMicrophone}>{t("general.request")}</button>
          {:else if perms?.microphone === "denied"}<button type="button" class="btn quiet" onclick={() => openPane("microphone")}>{t("general.open_system_settings")}</button>{/if}
        </Row>
      </div>
      {#if hasFn}<p class="note">{t("general.fn_key_note")}</p>{/if}
    {:else}
      <h1>{t("welcome.try.title")}</h1>
      <p class="sub">{t("welcome.try.desc")}</p>

      <h2>{t("welcome.keys")}<span class="sp"></span><small>{t("welcome.keys_note")}</small></h2>
      <div class="keys">{#each shortcuts as k, i (i)}<span class="key">{k}</span>{/each}</div>

      <h2>{t("welcome.result")}</h2>
      <div class="try">
        <div class="hint">
          <!-- The widget's ring: the mark at rest, a full ring while listening, a turning gap while it works. -->
          <span class="orb" class:accent={listening || busyRing} class:spin={busyRing} class:mark={!listening && !busyRing} aria-hidden="true">
            <svg viewBox="0 0 20 20"><circle class="arc" cx="10" cy="10" r="8.5" pathLength="360" />{#if !listening && !busyRing}<circle class="dot" cx="16.01" cy="3.99" r="1.8" />{/if}</svg>
          </span>
          <span>
            {#if dictation === "recording" || dictation === "locked"}{t("welcome.result.recording")}
            {:else if dictation === "processing" || dictation === "inserting"}{t("welcome.result.processing")}
            {:else if noEngine}{t("welcome.result.no_engine")}
            {:else if downloading}{t("welcome.result.waiting_models", { done: formatBytes(totals.received), total: formatBytes(totals.total) })}
            {:else}{t("welcome.result.ready")}{/if}
          </span>
        </div>
        {#if last}
          <dl class="out">
            <dt>{t("welcome.result.asr")}</dt><dd class="muted">{last.raw_text}</dd>
            <dt>{t("welcome.result.final")}</dt><dd>{last.final_text}</dd>
            <dt>{t("welcome.result.time")}</dt><dd class="muted small">{last.llm_ms === null ? t("welcome.result.time_asr", { asr: secs(last.asr_ms) }) : t("welcome.result.time_both", { asr: secs(last.asr_ms), llm: secs(last.llm_ms) })}</dd>
          </dl>
        {/if}
      </div>

      <h2>{t("welcome.launch")}</h2>
      <div class="group">
        <Row label={t("general.launch_at_login")} help={t(guide.launchAtLogin ? "welcome.launch_on" : "welcome.launch_off")}>
          <Switch bind:checked={guide.launchAtLogin} label={t("general.launch_at_login")} />
        </Row>
      </div>
    {/if}
    </div>
  </main>

  <footer>
    {#if guide.step === 1 && !guide.started && client.value}<LanguagePicker value={client.value.ui_language} onchange={setLanguage} />{/if}
    {#if guide.step > 1 && downloading}
      <span class="dl"><span class="bar"><i style:width="{totals.total ? (totals.received / totals.total) * 100 : 0}%"></i></span>{t("welcome.downloading", { done: formatBytes(totals.received), total: formatBytes(totals.total) })}</span>
    {/if}
    <span class="sp"></span>
    {#if guide.step > 1}<button type="button" class="btn quiet" onclick={back}>{t("welcome.back")}</button>{/if}
    {#if guide.step < 3}<button type="button" class="btn primary big" disabled={!canContinue} onclick={next}>{t("welcome.continue")}</button>
    {:else}<button type="button" class="btn primary big" onclick={finish}>{t("welcome.done")}</button>{/if}
  </footer>
</div>

<style>
  .guide { position: relative; height: 100vh; display: flex; flex-direction: column; background: var(--page); }
  .titlebar { flex: none; height: var(--titlebar); }
  header { flex: none; display: flex; flex-direction: column; align-items: center; gap: 18px; padding-bottom: 8px; }
  .brand { display: flex; align-items: center; gap: 9px; font: 800 24px/1 var(--font-round); letter-spacing: -.035em; }
  .brand em { font-style: normal; background: linear-gradient(315deg, var(--brand-1), var(--brand-2) 52%, var(--brand-3)); -webkit-background-clip: text; background-clip: text; color: transparent; }
  .steps { display: flex; align-items: center; gap: 10px; margin: 0; padding: 0; list-style: none; font-size: var(--text-control); color: var(--ink-3); }
  .steps li { display: inline-flex; align-items: center; gap: 7px; font-weight: 600; }
  .steps li i { display: inline-flex; width: 22px; height: 22px; border-radius: 50%; border: 1.5px solid var(--line-strong); align-items: center; justify-content: center; font-size: var(--text-xs); font-style: normal; }
  .steps li.on { color: var(--ink); } .steps li.on i { background: var(--accent); color: var(--accent-ink); border-color: var(--accent); }
  .steps li.done { color: var(--ink-2); } .steps li.done i { background: color-mix(in srgb, var(--ok) 14%, transparent); color: var(--ok); border-color: transparent; }
  .steps .dash { width: 28px; height: 1px; background: var(--line-strong); }
  main { flex: 1; min-height: 0; overflow-y: auto; overflow-anchor: none; padding: 16px var(--page-x) 20px; }
  .pg { width: 720px; margin: 0 auto; }
  h1 { margin: 0; font: 700 28px/1.2 var(--font-round); }
  .sub { margin: 4px 0 2px; color: var(--ink-2); font-size: var(--text-control); }
  h2 { display: flex; align-items: center; gap: 8px; margin: 14px 0 8px; padding: 0 4px; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); }
  h2 small { font-weight: 500; color: var(--ink-3); font-size: var(--text-sm); }
  .sp { flex: 1; }
  .cards { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .pick { position: relative; display: flex; gap: 12px; align-items: flex-start; width: 100%; min-height: 84px; padding: 13px 16px; border: 1px solid var(--line); border-radius: 14px; background: var(--surface); color: var(--ink); text-align: left; font: inherit; cursor: default; }
  .pick.on { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent) inset; }
  .pick.dim { opacity: .55; }
  .pick:disabled { pointer-events: none; }
  .pick.wide { min-height: 0; align-items: center; margin-top: 10px; }
  .dot { flex: none; width: 18px; height: 18px; margin-top: 2px; border-radius: 50%; border: 1.5px solid var(--line-strong); background: var(--surface); }
  .on .dot { border: 5.5px solid var(--accent); }
  .wide .dot { margin-top: 0; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .name { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-weight: 600; }
  .size { color: var(--ink-3); font-variant-numeric: tabular-nums; margin-right: 8px; }
  .pick small { color: var(--ink-2); font-size: var(--text-sm); }
  .bar { display: block; height: 6px; margin-top: 8px; border-radius: 3px; background: var(--sunken); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); transition: width .2s; }
  .pst { font-size: var(--text-xs); color: var(--ink-3); font-variant-numeric: tabular-nums; }
  .more { display: flex; justify-content: center; margin-top: 6px; }
  .act { display: flex; align-items: center; gap: 12px; margin-top: 18px; }
  .status { flex: 1; color: var(--ink-2); font-size: var(--text-control); font-variant-numeric: tabular-nums; }
  .err { color: var(--live); }
  .group { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); overflow: hidden; margin-top: 8px; }
  .note { margin: 14px 0 0; padding: 0 4px; color: var(--ink-3); font-size: var(--text-sm); }
  .keys { display: flex; gap: 8px; flex-wrap: wrap; }
  .key { display: inline-flex; align-items: center; height: 30px; padding: 0 12px; border-radius: 8px; background: var(--sunken); border: 1px solid var(--line); font-size: var(--text-control); font-weight: 600; }
  .try { background: var(--surface); border: 1px solid var(--line); border-radius: 14px; padding: 16px; }
  .hint { display: flex; align-items: center; gap: 12px; color: var(--ink-2); font-size: var(--text-control); }
  /* Drawn as the widget draws it: a line of 3 on 20 units, the gap of the mark at the upper right. */
  .orb { flex: none; width: 44px; height: 44px; color: var(--ink-3); }
  .orb svg { width: 100%; height: 100%; overflow: visible; }
  .orb .arc { fill: none; stroke: currentColor; stroke-width: 3; stroke-dasharray: 360 0; transform-origin: 10px 10px; transform: rotate(-3deg); }
  .orb .dot { fill: currentColor; }
  .orb.mark .arc { stroke-dasharray: 276 84; stroke-linecap: round; }
  .orb.accent { color: var(--accent); }
  .orb.spin { animation: orb-spin .9s linear infinite; }
  .orb.spin .arc { stroke-dasharray: 270 90; stroke-linecap: round; transform: rotate(45deg); }
  @keyframes orb-spin { to { transform: rotate(360deg); } }
  .out { display: grid; grid-template-columns: 56px 1fr; gap: 6px 12px; margin: 14px 0 0; padding-top: 12px; border-top: 1px solid var(--line); }
  .out dt { color: var(--ink-3); font-size: var(--text-sm); padding-top: 3px; }
  .out dd { margin: 0; line-height: 1.6; overflow-wrap: anywhere; }
  .muted { color: var(--ink-2); } .small { font-size: var(--text-sm); }
  footer { flex: none; display: flex; align-items: center; gap: 10px; padding: 16px 32px 24px; }
  .dl { display: flex; align-items: center; gap: 10px; color: var(--ink-3); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }
  .dl .bar { width: 120px; margin: 0; }
  .btn.big { height: 38px; padding: 0 18px; font-size: var(--text); }
</style>
