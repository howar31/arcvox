<script lang="ts">
  import { onMount } from "svelte";
  import { uiLanguage } from "../lib/api";
  import { setLanguage, t, type Lang } from "../lib/i18n";
  import { client, config, pageFromHash, PAGES, type Page } from "../lib/store.svelte";
  import General from "./pages/General.svelte";
  import Providers from "./pages/Providers.svelte";
  import Prompt from "./pages/Prompt.svelte";
  import About from "./pages/About.svelte";
  import Placeholder from "./pages/Placeholder.svelte";

  let page = $state<Page>(pageFromHash(location.hash));
  let ready = $state(false);

  onMount(() => {
    const onHash = () => { page = pageFromHash(location.hash); };
    window.addEventListener("hashchange", onHash);
    (async () => {
      setLanguage((await uiLanguage()) as Lang);
      await Promise.all([config.load(), client.load()]);
      await config.watch();
      ready = true;
    })();
    return () => window.removeEventListener("hashchange", onHash);
  });

  function go(p: Page) { location.hash = p; }
</script>

<div class="shell">
  <nav>
    {#each PAGES as p}
      <button class:active={page === p} onclick={() => go(p)}>{t(`nav.${p}`)}</button>
    {/each}
  </nav>
  <main>
    {#if !ready}
      <p class="muted">{t("common.loading")}</p>
    {:else if page === "general"}<General />
    {:else if page === "providers"}<Providers />
    {:else if page === "prompt"}<Prompt />
    {:else if page === "about"}<About />
    {:else}<Placeholder name={page} />
    {/if}
  </main>
</div>

<style>
  .shell { display: grid; grid-template-columns: 180px 1fr; height: 100vh; }
  nav { display: flex; flex-direction: column; gap: 2px; padding: 12px 8px; border-right: 1px solid var(--border); }
  nav button { text-align: left; padding: 6px 10px; border-radius: 6px; border: 0; background: transparent; color: inherit; font: inherit; }
  nav button.active { background: var(--accent); color: white; }
  main { padding: 20px 24px; overflow: auto; }
</style>
