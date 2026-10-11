<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { onProblemPages, problemPages, systemInfo, uiLanguage, type SystemInfo } from "../lib/api";
  import { setLanguage, t, type Lang } from "../lib/i18n";
  import { client, config, defaults } from "../lib/store.svelte";
  import { go, NAV, navPage, parseRoute, routeHash, routeSection, SETTINGS_PAGES, shouldIntercept, type SettingsPage } from "../lib/route";
  import { guard } from "../lib/guard.svelte";
  import { pause } from "../lib/pause.svelte";
  import Icon from "../lib/ui/Icon.svelte";
  import Mark from "../lib/ui/Mark.svelte";
  import Ring from "../lib/ui/Ring.svelte";
  import Modal from "../lib/ui/Modal.svelte";
  import Toasts from "../lib/ui/Toasts.svelte";
  import Home from "./pages/Home.svelte";
  import General from "./pages/General.svelte";
  import Engines from "./pages/Engines.svelte";
  import Flow from "./pages/Flow.svelte";
  import Prompt from "./pages/Prompt.svelte";
  import Dictionary from "./pages/Dictionary.svelte";
  import History from "./pages/History.svelte";
  import Speed from "./pages/Speed.svelte";
  import About from "./pages/About.svelte";
  import Licenses from "./pages/Licenses.svelte";
  import Backup from "./pages/Backup.svelte";

  let route = $state(parseRoute(location.hash));
  // Nothing translated is rendered before the UI language is known.
  let langReady = $state(false);
  let ready = $state(false);
  // The version of this build, at the foot of the sidebar.
  let info = $state<SystemInfo | null>(null);
  let main: HTMLElement | undefined = $state();
  // Shown after a first press of the quit shortcut, for as long as a second press quits.
  let quitHint = $state(false);
  let quitTimer: ReturnType<typeof setTimeout> | undefined;
  const current = $derived(navPage(route.page));
  const section = $derived((SETTINGS_PAGES as readonly string[]).includes(route.page) ? (route.page as SettingsPage) : null);

  // The pages that have a problem to show; the menu-bar menu lists the same problems.
  let problems = $state<string[]>([]);

  function checkProblems() {
    problemPages().then((pages) => { problems = pages; }).catch(() => {});
  }

  /** How long a section stays marked after a link led to it. */
  const ARRIVED_MS = 1600;
  let arrivedTimer: ReturnType<typeof setTimeout> | undefined;

  /** Brings the section a hash names into view, once the page that holds it is there. */
  function reveal(hash: string) {
    const name = routeSection(hash);
    if (!name) return;
    const find = () => main?.querySelector<HTMLElement>(`[data-section="${CSS.escape(name)}"]`);
    tick().then(() => {
      const el = find();
      if (!el) return;
      el.scrollIntoView({ block: "start" });
      // Marked for a moment: a section that was already in view does not move.
      delete el.dataset.arrived;
      void el.offsetWidth;
      el.dataset.arrived = "";
      clearTimeout(arrivedTimer);
      arrivedTimer = setTimeout(() => { delete el.dataset.arrived; }, ARRIVED_MS);
    });
    // What loads after the page is first shown moves the section.
    setTimeout(() => find()?.scrollIntoView({ block: "start" }), 300);
  }

  onMount(() => { pause.watch(); });

  onMount(() => {
    const onHash = () => {
      const next = parseRoute(location.hash);
      // A change that did not go through the guard (the tray sets the hash directly) must not drop
      // unsaved edits: put the current route back and ask first.
      if (shouldIntercept(guard.dirty, route, next)) {
        const target = location.hash;
        history.replaceState(null, "", routeHash(route.page, route.arg));
        guard.request(() => { location.hash = target; });
        return;
      }
      if (next.page !== route.page) main?.scrollTo(0, 0);
      route = next;
      reveal(location.hash);
    };
    const onFocus = () => { checkProblems(); };
    window.addEventListener("hashchange", onHash);
    window.addEventListener("focus", onFocus);
    const quitArmed = listen<number>("quit-armed", (e) => {
      quitHint = true;
      clearTimeout(quitTimer);
      quitTimer = setTimeout(() => { quitHint = false; }, e.payload);
    });
    const problemsChanged = onProblemPages((pages) => { problems = pages; });
    systemInfo().then((i) => { info = i; }).catch(() => {});
    (async () => {
      setLanguage((await uiLanguage()) as Lang);
      langReady = true;
      await Promise.all([config.load(), client.load(), defaults.load()]);
      checkProblems();
      await config.watch();
      await client.watch();
      ready = true;
      reveal(location.hash);
    })();
    return () => {
      window.removeEventListener("hashchange", onHash);
      window.removeEventListener("focus", onFocus);
      quitArmed.then((off) => off());
      problemsChanged.then((off) => off());
      clearTimeout(quitTimer);
    };
  });
</script>

{#if !ready}
  <div class="loading">{#if langReady}{t("common.loading")}{/if}</div>
{:else}
  <div class="shell">
    <aside class="sidebar">
      <div class="titlebar" data-tauri-drag-region></div>
      <div class="brand"><Mark size={25} /><span><em>A</em>rcvox</span></div>
      <nav aria-label={t("app.title")}>
        {#each NAV as g, gi (gi)}
          {#if g.group}<div class="nav-group">{t(g.group)}</div>{:else if gi > 0}<div class="nav-gap"></div>{/if}
          {#each g.pages as p (p)}
            <button
              type="button"
              class="nav"
              class:on={current === p}
              aria-current={current === p ? "page" : undefined}
              onclick={() => guard.request(() => go(p))}
            ><Icon name={p} size={18} /><span>{t(`nav.${p}`)}</span>{#if problems.includes(p)}<span class="bang" role="img" aria-label={t("shell.page_problem")} title={t("shell.page_problem")}>!</span>{/if}</button>
          {/each}
        {/each}
      </nav>
      {#if info}
        <div class="version">{info.name} {info.version}</div>
      {/if}
    </aside>

    <main class="main" bind:this={main}>
      <div class="titlebar main-titlebar" data-tauri-drag-region></div>
      <div class="page" class:wide={route.page === "home" || route.page === "history"}>
        {#if config.error}
          <div class="banner" role="alert"><Ring tone="live" /><span>{t("common.engine_error", { detail: config.error })}</span></div>
        {/if}
        {#if pause.value}
          <div class="banner paused" role="status"><span>{t("common.paused")}</span><button type="button" class="btn" onclick={() => pause.resume().catch(() => {})}>{t("common.resume")}</button></div>
        {/if}
        {#if route.page === "home"}<Home />
        {:else if section}{#key section}<General {section} />{/key}
        {:else if route.page === "about"}<About />
        {:else if route.page === "licenses"}<Licenses arg={route.arg} />
        {:else if route.page === "backup"}<Backup />
        {:else if route.page === "engines"}<Engines arg={route.arg} />
        {:else if route.page === "flow"}<Flow arg={route.arg} />
        {:else if route.page === "prompt"}<Prompt />
        {:else if route.page === "dictionary"}<Dictionary />
        {:else if route.page === "history"}<History />
        {:else if route.page === "speed"}<Speed />
        {/if}
      </div>
    </main>
  </div>

  <Modal bind:open={guard.pending} title={t("common.discard_title")} onclose={() => guard.cancel()}>
    <p>{t("common.discard_body")}</p>
    {#snippet actions()}
      <button type="button" class="btn" onclick={() => guard.cancel()}>{t("common.stay")}</button>
      <button type="button" class="btn primary" onclick={() => guard.confirm()}>{t("common.discard")}</button>
    {/snippet}
  </Modal>
  <Toasts />
{/if}
{#if quitHint && langReady}
  <div class="quit-hint" role="status">{t("shell.quit_again")}</div>
{/if}

<style>
  .loading { height: 100vh; display: grid; place-items: center; color: var(--ink-2); }
  .shell { display: grid; grid-template-columns: 248px minmax(0, 1fr); height: 100vh; }
  .sidebar { background: var(--sidebar); border-right: 1px solid var(--line); padding: 0 14px 14px; display: flex; flex-direction: column; gap: 3px; min-height: 0; overflow-y: auto; }
  /* The title bar is transparent and the content runs behind it; this band keeps the window controls clear and moves the window. */
  .titlebar { flex: none; height: var(--titlebar); }
  .main-titlebar { position: absolute; inset: 0 0 auto; }
  .brand { display: flex; align-items: center; gap: 8.5px; padding: 0 12px 18px; font: 800 21px/1 var(--font-round); letter-spacing: -.035em; }
  .brand em { font-style: normal; background: linear-gradient(315deg, var(--brand-1), var(--brand-2) 52%, var(--brand-3)); -webkit-background-clip: text; background-clip: text; color: transparent; }
  nav { display: flex; flex-direction: column; gap: 3px; }
  .nav-group { font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); padding: 14px 12px 6px; }
  .nav-gap { height: 12px; flex: none; }
  .nav { display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 12px; border-radius: 10px; font-size: var(--text); border: 0; background: none; color: var(--ink); text-align: left; cursor: default; white-space: nowrap; }
  .nav span { overflow: hidden; text-overflow: ellipsis; }
  .nav :global(.icon) { opacity: .75; }
  /* Out of the row's flow and close to its edge: the name starts where the others do, and the longest names keep their full length. */
  .nav:has(.bang) { position: relative; padding-right: 28px; }
  .nav .bang { position: absolute; right: 7px; top: 50%; transform: translateY(-50%); width: 16px; height: 16px; border-radius: 50%; background: var(--live); color: #fff; font: 700 11px/16px var(--font-round); text-align: center; overflow: visible; }
  .nav:hover { background: var(--line); }
  .nav.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .nav.on :global(.icon) { opacity: 1; }
  .version { margin-top: auto; padding: 16px; font-size: 11.5px; font-weight: 500; color: var(--ink-3); white-space: nowrap; }
  /* Positioned: absolutely placed descendants (visually hidden labels) stay inside the scroll area. */
  /* No scroll anchoring: a fold opening below the view must not drag the view along with it. */
  .main { position: relative; overflow-x: hidden; overflow-y: auto; overflow-anchor: none; min-width: 0; }
  /* Centered once the window is wider than the content; pages can lay themselves out by this width. */
  .page { padding: var(--page-top) var(--page-x) 80px; --page-max: 880px; max-width: var(--page-max); min-height: 100%; margin: 0 auto; container-type: inline-size; }
  /* Pages that show data use more of a wide window than pages of settings. */
  .page.wide { --page-max: 1320px; }
  .banner { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 22px; padding: 12px var(--row-x); border-radius: var(--radius-group); background: color-mix(in srgb, var(--live) 10%, var(--surface)); border: 1px solid color-mix(in srgb, var(--live) 35%, transparent); }
  .quit-hint { position: fixed; left: 50%; top: 50%; transform: translate(-50%, -50%); z-index: 80; padding: 14px 24px; border-radius: 14px; background: var(--pill-bg); color: var(--pill-ink); font-size: var(--text-lg); font-weight: 600; white-space: nowrap; box-shadow: var(--shadow); pointer-events: none; }
  .banner :global(.ring) { margin-top: 3px; }
  .banner.paused { align-items: center; justify-content: space-between; background: color-mix(in srgb, var(--accent) 10%, var(--surface)); border-color: color-mix(in srgb, var(--accent) 35%, transparent); }
</style>
