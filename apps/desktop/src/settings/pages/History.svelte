<script lang="ts">
  import { onMount } from "svelte";
  import {
    chooseAttempt, clearHistory, deleteAttempt, historyAttempts, clearRecordings, deleteHistory, deleteRecording, errorMessage, formatBytes, historyStorage, listHistory, onHistoryChanged, openRecordingsDir, reprocessHistory, revealRecording, widgetCopy,
    type Attempt, type HistoryStorage, type Utterance,
  } from "../../lib/api";
  import { failureText } from "../../lib/failure";
  import { formatDateTime, formatDay, formatTime } from "../../lib/format";
  import { clipLength, groupByDay, rawState, redoneTag, seconds, totalSeconds } from "../../lib/historyView";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import { firstShortcut } from "../../lib/shortcut";
  import { client, config } from "../../lib/store.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import Player from "./history/Player.svelte";

  /** How long typing pauses before the search runs. */
  const SEARCH_DELAY_MS = 250;
  const CHANGE_DELAY_MS = 400;

  let rows = $state<Utterance[] | null>(null);
  let storage = $state<HistoryStorage | null>(null);
  let now = $state(new Date());
  let confirmClear = $state(false);
  let confirmRecordings = $state(false);
  // Set when the list could not be read and nothing has been shown yet.
  let failed = $state(false);
  let query = $state("");
  let filter = $state<"all" | "audio">("all");
  // The expanded row, and whether its recording starts playing when it opens.
  let open = $state<number | null>(null);
  let autoplay = $state(false);
  // Bumped by each press of a play pill, so pressing it on an open row starts the recording again.
  let plays = $state(0);
  let timer: ReturnType<typeof setTimeout> | undefined;
  // Per row: the profile chosen for processing it again ("" is the row's own), whether that is running,
  // every result it had (loaded when the row opens), and the result just made that waits to be chosen.
  let chosen = $state<Record<number, string>>({});
  let busy = $state<Record<number, boolean>>({});
  let attempts = $state<Record<number, Attempt[]>>({});
  let fresh = $state<Record<number, number>>({});
  // The result whose delete button was pressed once; a second press within a few seconds deletes it.
  let confirming = $state<number | null>(null);
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;
  function askDelete(a: Attempt) {
    clearTimeout(confirmTimer);
    confirming = a.id;
    confirmTimer = setTimeout(() => { if (confirming === a.id) confirming = null; }, 4000);
  }

  async function reload() {
    clearTimeout(timer);
    try {
      rows = await listHistory(200, query, filter === "audio");
      now = new Date();
      failed = false;
    } catch (e) {
      if (rows === null) failed = true;
      toast.error(errorMessage(e));
    }
    historyStorage().then((s) => { storage = s; }).catch(() => { storage = null; });
  }

  onMount(() => {
    reload();
    const onFocus = () => { reload(); };
    window.addEventListener("focus", onFocus);
    // A dictation that ended while the page is shown. The row of a dictation still being
    // finished is not listed yet, so the list is read a moment later.
    let disposed = false;
    let unlisten: (() => void) | undefined;
    onHistoryChanged(() => {
      clearTimeout(timer);
      timer = setTimeout(reload, CHANGE_DELAY_MS);
    }).then((u) => { if (disposed) u(); else unlisten = u; });
    return () => { disposed = true; unlisten?.(); window.removeEventListener("focus", onFocus); clearTimeout(timer); };
  });

  function search() {
    clearTimeout(timer);
    timer = setTimeout(reload, SEARCH_DELAY_MS);
  }
  function setFilter(v: "all" | "audio") {
    filter = v;
    reload();
  }

  const filtered = $derived(query.trim() !== "" || filter === "audio");
  const groups = $derived(groupByDay(rows ?? [], now));
  function dayTitle(key: string): string {
    if (key === "today" || key === "yesterday") return t(`history.${key}`);
    const [y, m, d] = key.split("-").map(Number);
    return formatDay(new Date(y, m - 1, d), now);
  }
  const retention = $derived.by(() => {
    const g = config.value?.general;
    if (!g) return undefined;
    return g.keep_recordings
      ? t("history.retention", { days: String(g.history_retention_days), rec: String(g.recording_retention_days) })
      : t("history.retention_no_rec", { days: String(g.history_retention_days) });
  });

  /** The profile's name; for a profile that no longer exists, its id and a deleted marker. */
  function profileOf(id: string): { name: string; deleted: boolean } {
    const p = config.value?.profiles.find((x) => x.id === id);
    return p ? { name: p.name, deleted: false } : { name: id, deleted: !!config.value && !!id };
  }
  /** How long the processing took, in words. */
  function timing(u: { asr_ms: number; llm_ms: number | null }): string | null {
    const asr = seconds(u.asr_ms);
    const llm = seconds(u.llm_ms);
    const total = totalSeconds(u.asr_ms, u.llm_ms);
    if (asr !== null && llm !== null && total !== null) return t("history.total_secs", { total, asr, llm });
    return asr !== null ? t("history.asr_secs", { secs: asr }) : null;
  }

  /** The profile a row is processed with again: the chosen one, else its own, else the default. */
  /** The profile the dictation was made with: the original result's when another result is in use. */
  function ownProfileId(u: Utterance): string {
    return attempts[u.id]?.find((a) => a.origin)?.profile_id ?? u.profile_id;
  }
  function redoProfile(u: Utterance) {
    const profiles = config.value?.profiles ?? [];
    return profiles.find((p) => p.id === (chosen[u.id] || ownProfileId(u))) ?? profiles.find((p) => p.is_default) ?? null;
  }
  /** The engines of a profile, in the order they run. */
  function enginesOf(asr: string, llm: string | null): string {
    return llm ? `${asr} → ${llm}` : asr;
  }
  function redoEngines(u: Utterance): string {
    const p = redoProfile(u);
    if (!p) return "";
    const label = (id: string | null) => (id ? (config.value?.providers.find((x) => x.id === id)?.label ?? id) : null);
    const rewrites = p.steps.some((s) => s.step === "llm");
    return enginesOf(label(p.asr_provider) ?? "", rewrites ? label(p.llm_provider) : null);
  }
  /** The results a row had, newest first. */
  async function loadAttempts(id: number) {
    try { attempts[id] = await historyAttempts(id); } catch (e) { toast.error(errorMessage(e)); }
  }
  /** Processes the row's recording again. The result joins the row's results; a failed row takes it at once. */
  async function redo(u: Utterance) {
    busy[u.id] = true;
    delete fresh[u.id];
    try {
      const { attempt, failure } = await reprocessHistory(u.id, chosen[u.id] || null);
      if (failure) toast.error(failureText(failure.code, failure.detail));
      else if (attempt?.chosen) toast.ok(t("history.retry_done"));
      else if (attempt) fresh[u.id] = attempt.id;
    } catch (e) {
      toast.error(errorMessage(e));
    }
    delete busy[u.id];
    await Promise.all([reload(), loadAttempts(u.id)]);
  }
  async function use(u: Utterance, a: Attempt) {
    try {
      await chooseAttempt(u.id, a.id);
      if (fresh[u.id] === a.id) delete fresh[u.id];
      toast.ok(t("history.result_used"));
    } catch (e) {
      toast.error(errorMessage(e));
    }
    await Promise.all([reload(), loadAttempts(u.id)]);
  }
  async function removeAttempt(u: Utterance, a: Attempt) {
    clearTimeout(confirmTimer);
    confirming = null;
    try {
      await deleteAttempt(a.id);
      if (fresh[u.id] === a.id) delete fresh[u.id];
    } catch (e) {
      toast.error(errorMessage(e));
    }
    await Promise.all([reload(), loadAttempts(u.id)]);
  }
  /** When a result was made, for its line: the row's own time for the origin, "just now" for the one just made. */
  function attemptWhen(u: Utterance, a: Attempt): string {
    if (fresh[u.id] === a.id) return t("history.attempt_just_now");
    return formatTime(a.ts);
  }

  function toggle(id: number, play = false) {
    if (open === id && !play) { open = null; return; }
    autoplay = play;
    if (play) plays++;
    if (open !== id) {
      open = id;
      delete fresh[id];
      const row = rows?.find((u) => u.id === id);
      if (row?.redone) loadAttempts(id);
      else delete attempts[id];
    }
  }

  async function copy(text: string) {
    try { await widgetCopy(text); toast.ok(t("home.copied")); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function remove(id: number) {
    try { await deleteHistory(id); } catch (e) { toast.error(errorMessage(e)); }
    if (open === id) open = null;
    await reload();
  }
  async function removeRecording(id: number) {
    try { await deleteRecording(id); toast.ok(t("history.recording_deleted")); } catch (e) { toast.error(errorMessage(e)); }
    await reload();
  }
  async function openFolder() {
    try { await openRecordingsDir(); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function reveal(id: number) {
    try { await revealRecording(id); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function clearAll() {
    confirmClear = false;
    try {
      await clearHistory();
      toast.ok(t("history.cleared"));
    } catch (e) {
      toast.error(errorMessage(e));
    }
    open = null;
    await reload();
  }
  async function clearAllRecordings() {
    confirmRecordings = false;
    try {
      const n = await clearRecordings();
      toast.ok(t("history.recordings_cleared", { n: String(n) }));
    } catch (e) {
      toast.error(errorMessage(e));
    }
    await reload();
  }
</script>

<PageHead title={t("nav.history")} description={retention}>
  {#snippet actions()}
    <button type="button" class="btn danger" disabled={!storage?.rows} onclick={() => (confirmClear = true)}>{t("history.clear_all")}</button>
  {/snippet}
</PageHead>

{#if failed}
  <Group>
    <Empty text={t("history.load_failed")}>
      {#snippet action()}<button type="button" class="btn" onclick={reload}>{t("history.retry")}</button>{/snippet}
    </Empty>
  </Group>
{:else if rows !== null}
  {#if storage}
    <div class="summary">
      <span>{t("history.summary.rows", { n: storage.rows.toLocaleString() })}</span>
      <span>{t("history.summary.recordings", { n: storage.recordings.toLocaleString() })}</span>
      {#if storage.recordings > 0}<span>{t("history.summary.size", { size: formatBytes(storage.recording_bytes) })}</span>{/if}
      <span class="grow"></span>
      <button type="button" class="btn quiet" onclick={openFolder}>{t("general.recordings_dir_open")}</button>
      {#if storage.recordings > 0}<button type="button" class="btn quiet" onclick={() => (confirmRecordings = true)}>{t("history.clear_recordings")}</button>{/if}
    </div>
  {/if}
  <div class="tools">
    <label class="search">
      <Icon name="search" />
      <input
        type="search" bind:value={query} placeholder={t("history.search")} aria-label={t("history.search")}
        oninput={search} onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) reload(); }}
      />
    </label>
    <Segmented
      options={[{ value: "all", label: t("history.filter.all") }, { value: "audio", label: t("history.filter.with_audio") }]}
      bind:value={() => filter, setFilter} label={t("history.filter.label")}
    />
  </div>

  {#each groups as g (g.key)}
    <Group title={dayTitle(g.key)}>
      {#each g.rows as u (u.id)}
        {@const who = profileOf(u.profile_id)}
        <div class="entry" class:open={open === u.id}>
          <div class="line">
            <button type="button" class="main" aria-expanded={open === u.id} onclick={() => toggle(u.id)}>
              <span class="time" title={formatDateTime(u.ts)}>{formatTime(u.ts)}</span>
              <span class="body">
                <span class="final" class:none={u.error !== null}>{u.error !== null ? t("history.failed") : u.final_text}</span>
                <span class="meta">
                  {#if u.app_name}<span>{u.app_name}</span>{/if}
                  {#if u.profile_id}<span>{who.name}{#if who.deleted}<span class="deleted">{t("history.deleted_profile")}</span>{/if}</span>{/if}
                  {#if u.error !== null}<Tag tone="live">{t("history.failed_tag")}</Tag>{/if}
                  {#if u.degraded}<Tag tone="warn">{t("history.not_rewritten")}</Tag>{/if}
                  {#if redoneTag(u.redone)}<Tag tone="accent">{redoneTag(u.redone)}</Tag>{/if}
                </span>
              </span>
            </button>
            <span class="acts">
              {#if u.has_audio}
                <button type="button" class="play" aria-label={t("history.play")} onclick={() => toggle(u.id, true)}><Icon name="play" size={12} /><span class="num">{clipLength(u.audio_ms ?? 0)}</span></button>
              {/if}
              <button type="button" class="btn quiet icon-btn" aria-label={t("widget.copy")} title={t("widget.copy")} disabled={u.error !== null} onclick={() => copy(u.final_text)}><Icon name="copy" /></button>
            </span>
          </div>
          {#if open === u.id}
            <div class="detail">
              {#if u.error !== null}
                <div class="label">{t("history.error_label")}</div>
                {@const reason = failureText(u.error_code, u.error)}
                <div class="problem">{reason}</div>
                {#if reason !== u.error}
                  <div class="label">{t("history.error_detail")}</div>
                  <div class="tech">{u.error}</div>
                {/if}
              {:else if rawState(u) !== "none"}
                {@const raw = rawState(u)}
                <div class="label">{t("history.raw_label")}{#if raw !== "changed"}<span class="same" class:kept={raw === "same"} class:bad={raw === "failed"}>{t(raw === "same" ? "history.raw_same" : raw === "failed" ? "history.not_rewritten" : "history.rewrite_off")}</span>{/if}</div>
                <div class="raw">{u.raw_text.trim()}</div>
              {/if}
              {#if u.has_audio}
                {@const failedRow = u.error !== null}
                <div class="label">{t("history.recording")}</div>
                {#key plays}<Player id={u.id} {autoplay} />{/key}
                <div class="label">{t(failedRow ? "history.retry" : "history.reprocess")}</div>
                {#if busy[u.id]}
                  <div class="busy" role="status"><span class="spin" aria-hidden="true"></span>{t("history.processing", { engines: redoEngines(u) })}</div>
                {:else}
                  <div class="again">
                    <select bind:value={() => chosen[u.id] ?? "", (v) => (chosen[u.id] = v)} aria-label={t("history.profile_choice")}>
                      <option value="">{t("history.own_profile", { name: profileOf(ownProfileId(u)).name })}</option>
                      {#each (config.value?.profiles ?? []).filter((p) => p.id !== ownProfileId(u)) as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
                    </select>
                    <button type="button" class="btn" class:primary={failedRow} onclick={() => redo(u)}>{t(failedRow ? "history.retry" : "history.reprocess")}</button>
                    <span class="hint">{t(failedRow ? "history.retry_help" : "history.reprocess_help")}</span>
                  </div>
                {/if}
              {/if}
              {#if attempts[u.id]?.length}
                <div class="label">{t("history.attempts")}</div>
                <div class="attempts">
                  {#each attempts[u.id] as a (a.id)}
                    {@const isFresh = fresh[u.id] === a.id}
                    <div class="attempt" class:chosen={a.chosen} class:fresh={isFresh}>
                      {#if !a.chosen}
                        {#if confirming === a.id}
                          <button type="button" class="btn sm danger sure" onclick={() => removeAttempt(u, a)}>{t("history.delete_attempt_confirm")}</button>
                        {:else}
                          <button type="button" class="x" aria-label={t("history.delete_attempt")} title={t("history.delete_attempt")} onclick={() => askDelete(a)}><Icon name="x" size={12} /></button>
                        {/if}
                      {/if}
                      <div class="head num">
                        <span class="when">{attemptWhen(u, a)}</span>
                        <span>{profileOf(a.profile_id).name}</span>
                        {#if a.asr_label}<span>{enginesOf(a.asr_label, a.llm_label)}</span>{/if}
                        {#if a.error === null && timing(a)}<span>{timing(a)}</span>{/if}
                        {#if a.chosen}<Tag tone="accent">{t("history.attempt_chosen")}</Tag>{/if}
                        {#if isFresh}<Tag tone="accent">{t("history.attempt_fresh")}</Tag>{/if}
                        {#if a.origin}<Tag>{t("history.attempt_origin")}</Tag>{/if}
                        {#if a.error !== null}<Tag tone="live">{t("history.attempt_failed")}</Tag>{/if}
                        {#if a.degraded}<Tag tone="warn">{t("history.not_rewritten")}</Tag>{/if}
                      </div>
                      {#if a.error !== null}
                        <div class="text problem">{failureText(a.error_code, a.error)}</div>
                      {:else}
                        <div class="text">{a.final_text}</div>
                        {#if !a.chosen}
                          <div class="choose">
                            <button type="button" class="btn quiet icon-btn" aria-label={t("history.copy_result")} title={t("history.copy_result")} onclick={() => copy(a.final_text)}><Icon name="copy" /></button>
                            <button type="button" class="btn" class:primary={isFresh} onclick={() => use(u, a)}>{t("history.use_result")}</button>
                          </div>
                        {/if}
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}
              <div class="foot">
                <span class="facts num">
                  {#if u.error === null && timing(u)}<span>{timing(u)}</span>{/if}
                  <span>{u.has_audio && u.audio_bytes !== null ? t("history.recording_size", { size: formatBytes(u.audio_bytes) }) : t("history.no_recording_redo")}</span>
                </span>
                {#if u.has_audio}
                  <button type="button" class="btn quiet" onclick={() => reveal(u.id)}>{t("history.reveal")}</button>
                  <button type="button" class="btn quiet danger" onclick={() => removeRecording(u.id)}>{t("history.delete_recording")}</button>
                {/if}
                <button type="button" class="btn quiet danger" onclick={() => remove(u.id)}>{t("history.delete_row")}</button>
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </Group>
  {:else}
    <Group>
      <Empty text={filtered ? t("history.no_match") : t("history.empty", { key: firstShortcut(client.value?.hotkey.shortcuts) })} />
    </Group>
  {/each}
{/if}

<Modal bind:open={confirmClear} title={t("history.confirm_clear")}>
  <p>{t("history.clear_body")}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (confirmClear = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={clearAll}>{t("history.clear_all")}</button>
  {/snippet}
</Modal>

<Modal bind:open={confirmRecordings} title={t("history.clear_recordings_title")}>
  <p>{t("history.clear_recordings_body")}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (confirmRecordings = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={clearAllRecordings}>{t("history.clear_recordings")}</button>
  {/snippet}
</Modal>

<style>
  .summary { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 26px; white-space: nowrap; min-height: 58px; padding: 10px var(--row-x); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); font-size: var(--text-control); color: var(--ink-2); font-variant-numeric: tabular-nums; }
  .summary .grow { flex: 1; }
  .summary .btn { margin-right: -8px; }
  .tools { display: flex; align-items: center; gap: 12px; margin-top: 14px; }
  .search { flex: 1; min-width: 0; display: flex; align-items: center; gap: 9px; height: var(--control); padding: 0 12px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink-3); cursor: text; }
  .search:focus-within { outline: 2px solid var(--accent); outline-offset: 1px; }
  .search input { flex: 1; min-width: 0; height: 100%; padding: 0; border: 0; background: none; color: var(--ink); outline: none; -webkit-appearance: none; appearance: none; }
  .search input::-webkit-search-cancel-button { -webkit-appearance: none; }

  .entry + :global(.entry) { border-top: 1px solid var(--line); }
  .line { display: flex; align-items: flex-start; gap: 12px; padding-right: var(--row-x); }
  .main { flex: 1; min-width: 0; display: flex; align-items: flex-start; gap: 16px; padding: var(--row-y) 0 var(--row-y) var(--row-x); border: 0; background: none; text-align: left; cursor: default; }
  .main:focus-visible { outline-offset: -2px; }
  .entry:hover, .entry.open { background: color-mix(in srgb, var(--sunken) 55%, transparent); }
  .entry:first-child { border-top-left-radius: calc(var(--radius-group) - 1px); border-top-right-radius: calc(var(--radius-group) - 1px); }
  .entry:last-child { border-bottom-left-radius: calc(var(--radius-group) - 1px); border-bottom-right-radius: calc(var(--radius-group) - 1px); }
  .time { width: 52px; flex: none; padding-top: 3px; color: var(--ink-2); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .final { line-height: 1.55; overflow-wrap: anywhere; white-space: pre-line; }
  .meta { display: flex; align-items: center; gap: 2px 10px; flex-wrap: wrap; color: var(--ink-2); font-size: var(--text-sm); }
  .deleted { margin-left: 3px; color: var(--ink-3); }
  .acts { flex: none; display: flex; align-items: center; gap: 4px; padding-top: calc(var(--row-y) - 6px); margin-right: -8px; }
  .play { display: inline-flex; align-items: center; gap: 7px; height: 30px; padding: 0 12px 0 10px; border-radius: 15px; border: 1px solid var(--line-strong); background: var(--surface); font-size: var(--text-sm); cursor: default; }
  .play :global(.icon) { fill: currentColor; }
  .play:hover { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); }
  .icon-btn { width: var(--control); padding: 0; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }

  .detail { padding: 0 var(--row-x) 18px calc(var(--row-x) + 68px); }
  .label { margin: 10px 0 5px; font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); }
  .detail > .label:first-child { margin-top: 0; }
  .same { margin-left: 10px; font-weight: 400; }
  .same.bad { color: var(--warn); }
  .same.kept { color: var(--ok); }
  .raw { color: var(--ink-2); line-height: 1.55; overflow-wrap: anywhere; white-space: pre-line; -webkit-user-select: text; user-select: text; }
  .final.none { color: var(--ink-2); }
  .problem { color: var(--live); line-height: 1.55; overflow-wrap: anywhere; -webkit-user-select: text; user-select: text; }
  .tech { color: var(--ink-2); font: var(--text-sm)/1.5 var(--font-mono); overflow-wrap: anywhere; -webkit-user-select: text; user-select: text; }
  .again { display: flex; align-items: center; gap: 10px; }
  .again select { width: 240px; flex: none; }
  .again .btn { flex: none; }
  .hint { min-width: 0; color: var(--ink-2); font-size: var(--text-sm); }
  .busy { display: flex; align-items: center; gap: 9px; min-height: var(--control); color: var(--ink-2); font-size: var(--text-control); }
  .spin { width: 16px; height: 16px; flex: none; border-radius: 50%; border: 2.5px solid var(--accent); border-right-color: transparent; animation: spin .9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .attempts { display: flex; flex-direction: column; gap: 8px; }
  .attempt { position: relative; padding: 12px 16px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); }
  .attempt.chosen { border-color: var(--accent); background: var(--accent-soft); }
  .attempt.fresh { border-color: var(--accent); border-style: dashed; }
  .attempt .head { display: flex; align-items: center; flex-wrap: wrap; gap: 2px 12px; margin-bottom: 4px; padding-right: 28px; color: var(--ink-2); font-size: var(--text-sm); }
  .attempt .when { color: var(--ink); font-weight: 600; }
  .attempt .text { line-height: 1.55; overflow-wrap: anywhere; white-space: pre-line; -webkit-user-select: text; user-select: text; }
  .attempt .x { position: absolute; top: 8px; right: 8px; display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; padding: 0; border: 0; border-radius: 8px; background: none; color: var(--ink-3); cursor: default; }
  .attempt .x:hover { background: var(--sunken); color: var(--ink); }
  .attempt .sure { position: absolute; top: 6px; right: 8px; height: 28px; padding: 0 10px; font-size: var(--text-xs); }
  .choose { display: flex; align-items: center; justify-content: flex-end; gap: 8px; margin-top: 10px; }
  .foot { display: flex; align-items: center; gap: 4px; margin-top: 12px; }
  .foot .btn:last-child { margin-right: -8px; }
  .facts { flex: 1; min-width: 0; display: flex; flex-wrap: wrap; gap: 2px 14px; color: var(--ink-2); font-size: var(--text-sm); }
</style>
