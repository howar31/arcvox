<script lang="ts">
  import { onMount } from "svelte";
  import { permissions, uiLanguage } from "../lib/api";
  import { setLanguage, t, type Lang } from "../lib/i18n";
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
  import Profiles from "./pages/Profiles.svelte";
  import ProfileDetail from "./pages/ProfileDetail.svelte";
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
    };
  });
</script>

{#if !ready}
  <div class="loading">{#if langReady}{t("common.loading")}{/if}</div>
{:else}
  <div class="shell">
    <aside class="sidebar">
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
            ><Icon name={p} /><span>{t(`nav.${p}`)}</span></button>
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
          <Ring tone="ok" /><span>{t("shell.ready")}</span>
        {/if}
      </div>
    </aside>

    <main class="main" bind:this={main}>
      <div class="page">
        {#if config.error}
          <div class="banner" role="alert"><Ring tone="live" /><span>{t("common.engine_error", { detail: config.error })}</span></div>
        {/if}
        {#if route.page === "home"}<Home />
        {:else if route.page === "general"}<General />
        {:else if route.page === "about"}<About />
        {:else if route.page === "engines"}<Engines arg={route.arg} />
        {:else if route.page === "profiles"}
          {#if route.arg}{#key route.arg}<ProfileDetail arg={route.arg} />{/key}{:else}<Profiles />{/if}
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

<style>
  .loading { height: 100vh; display: grid; place-items: center; color: var(--ink-2); }
  .shell { display: grid; grid-template-columns: 196px minmax(0, 1fr); height: 100vh; }
  .sidebar { background: var(--sidebar); border-right: 1px solid var(--line); padding: 14px 10px; display: flex; flex-direction: column; gap: 2px; min-height: 0; overflow-y: auto; }
  nav { display: flex; flex-direction: column; gap: 2px; }
  .nav-group { font-size: 11px; font-weight: 600; color: var(--ink-2); padding: 14px 10px 4px; }
  .nav { display: flex; align-items: center; gap: 9px; height: 30px; padding: 0 10px; border-radius: 7px; border: 0; background: none; color: var(--ink); text-align: left; cursor: default; white-space: nowrap; }
  .nav span { overflow: hidden; text-overflow: ellipsis; }
  .nav :global(.icon) { opacity: .75; }
  .nav:hover { background: var(--line); }
  .nav.on { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .nav.on :global(.icon) { opacity: 1; }
  .state { margin-top: auto; display: flex; align-items: center; gap: 8px; padding: 10px; font-size: 12px; color: var(--ink-2); line-height: 1.35; }
  .state-link { border: 0; background: none; padding: 0; color: var(--ink-2); text-align: left; cursor: default; }
  .state-link:hover { text-decoration: underline; }
  /* Positioned: absolutely placed descendants (visually hidden labels) stay inside the scroll area. */
  .main { position: relative; overflow-x: hidden; overflow-y: auto; min-width: 0; }
  .page { padding: 26px 32px 80px; max-width: 664px; min-height: 100%; }
  .banner { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 20px; padding: 10px 14px; border-radius: 10px; background: color-mix(in srgb, var(--live) 10%, var(--surface)); border: 1px solid color-mix(in srgb, var(--live) 35%, transparent); }
  .banner :global(.ring) { margin-top: 3px; }
</style>
