<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { permissions, uiLanguage } from "../lib/api";
  import { setLanguage, t, type Lang } from "../lib/i18n";
  import { firstShortcut } from "../lib/shortcut";
  import { client, config } from "../lib/store.svelte";
  import { go, NAV, parseRoute, routeHash, shouldIntercept } from "../lib/route";
  import { guard } from "../lib/guard.svelte";
  import Icon from "../lib/ui/Icon.svelte";
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

  let route = $state(parseRoute(location.hash));
  // Nothing translated is rendered before the UI language is known.
  let langReady = $state(false);
  let ready = $state(false);
  let accessibility = $state<boolean | null>(null);
  let main: HTMLElement | undefined = $state();
  // Shown after a first press of the quit shortcut, for as long as a second press quits.
  let quitHint = $state(false);
  let quitTimer: ReturnType<typeof setTimeout> | undefined;

  async function checkPermissions() {
    try { accessibility = (await permissions()).accessibility; } catch { accessibility = null; }
  }

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
    };
    const onFocus = () => { checkPermissions(); };
    window.addEventListener("hashchange", onHash);
    window.addEventListener("focus", onFocus);
    const quitArmed = listen<number>("quit-armed", (e) => {
      quitHint = true;
      clearTimeout(quitTimer);
      quitTimer = setTimeout(() => { quitHint = false; }, e.payload);
    });
    (async () => {
      setLanguage((await uiLanguage()) as Lang);
      langReady = true;
      await Promise.all([config.load(), client.load(), checkPermissions()]);
      await config.watch();
      ready = true;
    })();
    return () => {
      window.removeEventListener("hashchange", onHash);
      window.removeEventListener("focus", onFocus);
      quitArmed.then((off) => off());
      clearTimeout(quitTimer);
    };
  });
</script>

{#if !ready}
  <div class="loading">{#if langReady}{t("common.loading")}{/if}</div>
{:else}
  <div class="shell">
    <aside class="sidebar">
      <div class="brand"><span class="mark"></span><span>Kongwe</span></div>
      <nav aria-label={t("app.title")}>
        {#each NAV as g, gi (gi)}
          {#if g.group}<div class="nav-group">{t(g.group)}</div>{/if}
          {#each g.pages as p (p)}
            <button
              type="button"
              class="nav"
              class:on={route.page === p}
              aria-current={route.page === p ? "page" : undefined}
              onclick={() => guard.request(() => go(p))}
            ><Icon name={p} size={18} /><span>{t(`nav.${p}`)}</span></button>
          {/each}
        {/each}
      </nav>
      <div class="state">
        {#if config.error}
          <Ring tone="live" /><span>{t("shell.engine_error")}</span>
        {:else if accessibility === false}
          <Ring tone="live" />
          <button type="button" class="state-link" onclick={() => guard.request(() => go("general"))}>{t("shell.needs_access")}</button>
        {:else if accessibility}
          <Ring tone="ok" /><span>{t("shell.ready", { key: firstShortcut(client.value?.hotkey.shortcuts) })}</span>
        {/if}
      </div>
    </aside>

    <main class="main" bind:this={main}>
      <div class="page" class:wide={route.page === "home" || route.page === "history"}>
        {#if config.error}
          <div class="banner" role="alert"><Ring tone="live" /><span>{t("common.engine_error", { detail: config.error })}</span></div>
        {/if}
        {#if route.page === "home"}<Home />
        {:else if route.page === "general"}<General />
        {:else if route.page === "about"}<About />
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
  .sidebar { background: var(--sidebar); border-right: 1px solid var(--line); padding: 18px 14px 14px; display: flex; flex-direction: column; gap: 3px; min-height: 0; overflow-y: auto; }
  .brand { display: flex; align-items: center; gap: 11px; padding: 6px 12px 18px; font: 600 21px/1 var(--font-round); letter-spacing: .01em; }
  .mark { width: 26px; height: 26px; border-radius: 50%; border: 6px solid var(--accent); flex: none; }
  nav { display: flex; flex-direction: column; gap: 3px; }
  .nav-group { font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); padding: 20px 12px 6px; }
  .nav { display: flex; align-items: center; gap: 12px; height: 46px; padding: 0 12px; border-radius: 10px; font-size: var(--text); border: 0; background: none; color: var(--ink); text-align: left; cursor: default; white-space: nowrap; }
  .nav span { overflow: hidden; text-overflow: ellipsis; }
  .nav :global(.icon) { opacity: .75; }
  .nav:hover { background: var(--line); }
  .nav.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .nav.on :global(.icon) { opacity: 1; }
  .state { margin-top: auto; display: flex; align-items: center; gap: 8px; padding: 12px; font-size: var(--text-sm); color: var(--ink-2); line-height: 1.35; }
  .state-link { border: 0; background: none; padding: 0; color: var(--ink-2); text-align: left; cursor: default; }
  .state-link:hover { text-decoration: underline; }
  /* Positioned: absolutely placed descendants (visually hidden labels) stay inside the scroll area. */
  .main { position: relative; overflow-x: hidden; overflow-y: auto; min-width: 0; }
  /* Centered once the window is wider than the content; pages can lay themselves out by this width. */
  .page { padding: var(--page-top) var(--page-x) 80px; max-width: 880px; min-height: 100%; margin: 0 auto; container-type: inline-size; }
  /* Pages that show data use more of a wide window than pages of settings. */
  .page.wide { max-width: 1320px; }
  .banner { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 22px; padding: 12px var(--row-x); border-radius: var(--radius-group); background: color-mix(in srgb, var(--live) 10%, var(--surface)); border: 1px solid color-mix(in srgb, var(--live) 35%, transparent); }
  .quit-hint { position: fixed; left: 50%; top: 50%; transform: translate(-50%, -50%); z-index: 80; padding: 14px 24px; border-radius: 14px; background: var(--pill-bg); color: var(--pill-ink); font-size: var(--text-lg); font-weight: 600; white-space: nowrap; box-shadow: var(--shadow); pointer-events: none; }
  .banner :global(.ring) { margin-top: 3px; }
</style>
