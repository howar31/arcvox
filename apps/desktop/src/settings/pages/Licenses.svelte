<script lang="ts">
  import { t } from "../../lib/i18n";
  import { go } from "../../lib/route";
  import { filterPackages, findPackage, licenseIds, packageKey, type ThirdPartyData, type ThirdPartyPackage } from "../../lib/thirdParty";
  import data from "../../generated/third-party.json";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Icon from "../../lib/ui/Icon.svelte";

  /** The package named in the route, or nothing for the list. */
  let { arg = null }: { arg?: string | null } = $props();

  const all = (data as ThirdPartyData).packages;
  const texts = (data as ThirdPartyData).texts;
  const APP: ThirdPartyPackage = { name: "Arcvox", version: "", source: "rust", license: "Apache-2.0", copyright: ["Copyright 2026 Howar31"] };
  const APP_KEY = "arcvox";
  const runtimes = ["whisper.cpp", "llama.cpp", "ggml"];
  const mitTerms = `Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;
  const mitNotice = `MIT License

Copyright (c) 2023-2024 The ggml authors (whisper.cpp)
Copyright (c) 2023-2026 The ggml authors (llama.cpp, ggml)

${mitTerms}`;
  // Reflow the hard-wrapped paragraphs so the notice fits any window width.
  const noticeText = mitNotice.replace(/([^\n])\n(?!\n|Copyright)/g, "$1 ");

  let query = $state("");
  const shown = $derived(filterPackages(all, query));
  const pkg = $derived(arg === APP_KEY ? APP : arg ? findPackage(all, arg) : undefined);
  const crumbs = $derived([{ label: t("nav.about"), page: "about" as const }, { label: t("about.licenses"), page: "licenses" as const }]);

  function sourceLabel(p: ThirdPartyPackage): string {
    return t(p.source === "rust" ? "licenses.source_rust" : "licenses.source_npm");
  }
  function subtitle(p: ThirdPartyPackage): string {
    return p === APP ? p.license : `${p.version} · ${sourceLabel(p)} · ${p.license}`;
  }
</script>

{#if pkg}
  <PageHead title={pkg.name} description={subtitle(pkg)} crumb={crumbs} />
  {#if pkg.copyright.length}
    <Group title={t("licenses.notice")}>
      <div class="lines">{#each pkg.copyright as line (line)}<div>{line}</div>{/each}</div>
    </Group>
  {/if}
  {#each licenseIds(pkg.license) as id (id)}
    <Group title={id}>
      <pre class="text">{texts[id] ?? t("licenses.no_text")}</pre>
    </Group>
  {/each}
{:else if arg}
  <PageHead title={arg} crumb={crumbs} />
  <p class="help">{t("licenses.none")}</p>
{:else}
  <PageHead title={t("about.licenses")} crumb={{ label: t("nav.about"), page: "about" }} />

  <Group title={t("licenses.app")}>
    <Row label="Arcvox" help={t("about.license")} onclick={() => go("licenses", APP_KEY)}>
      <span class="sub">Apache-2.0</span>
    </Row>
  </Group>

  <Group title={t("licenses.runtimes")} help={t("about.model_licenses")}>
    <details>
      <summary><span class="label"><span class="name">{runtimes.join(", ")}</span><small>{t("licenses.runtime_copyright")}</small></span><span class="sub">MIT</span><span class="chev"><Icon name="chevron-right" size={14} /></span></summary>
      <pre class="text">{noticeText}</pre>
    </details>
  </Group>

  <Group title={`${t("licenses.packages")} · ${all.length}`}>
    {#snippet actions()}
      <label class="search">
        <Icon name="search" />
        <input type="search" bind:value={query} placeholder={t("licenses.filter")} aria-label={t("licenses.filter")} />
      </label>
    {/snippet}
    {#each shown as p (packageKey(p, all))}
      <Row label={p.name} help={`${p.version} · ${sourceLabel(p)}`} onclick={() => go("licenses", packageKey(p, all))}>
        <span class="sub license">{p.license}</span>
      </Row>
    {:else}
      <p class="empty">{t("licenses.none")}</p>
    {/each}
    {#if shown.length === all.length}
      <p class="empty">{t("licenses.count", { count: String(all.length) })}</p>
    {/if}
  </Group>
{/if}

<style>
  .lines { padding: var(--row-y) var(--row-x); color: var(--ink-2); font-size: var(--text-sm); line-height: 1.6; overflow-wrap: anywhere; }
  .text { margin: 0; padding: 14px var(--row-x) 16px; white-space: pre-wrap; font: 12px/1.55 var(--font-mono); color: var(--ink-2); overflow-wrap: anywhere; }
  details .text { border-top: 1px solid var(--line); max-height: 260px; overflow: auto; }
  details summary { display: flex; align-items: center; gap: 10px; min-height: var(--row-min); padding: var(--row-y) var(--row-x); list-style: none; cursor: default; border-radius: calc(var(--radius-group) - 1px); }
  details summary::-webkit-details-marker { display: none; }
  details summary:hover { background: var(--sunken); }
  details summary:focus-visible { outline-offset: -2px; }
  summary .label { flex: 1; min-width: 0; }
  summary .name { display: block; }
  summary small { display: block; color: var(--ink-2); font-size: var(--text-sm); margin-top: 2px; }
  .chev { display: flex; color: var(--ink-3); transition: transform .15s; }
  details[open] .chev { transform: rotate(90deg); }
  details[open] summary { border-radius: calc(var(--radius-group) - 1px) calc(var(--radius-group) - 1px) 0 0; }
  .license { text-align: right; overflow-wrap: anywhere; }
  .search { display: flex; align-items: center; gap: 9px; width: 260px; height: var(--control); padding: 0 12px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink-3); cursor: text; }
  .search:focus-within { outline: 2px solid var(--accent); outline-offset: 1px; }
  .search input { flex: 1; min-width: 0; height: 100%; padding: 0; border: 0; background: none; color: var(--ink); outline: none; -webkit-appearance: none; appearance: none; }
  .search input::-webkit-search-cancel-button { -webkit-appearance: none; }
  .empty { margin: 0; padding: 14px var(--row-x); text-align: center; color: var(--ink-3); font-size: var(--text-sm); }
  .empty + :global(.item), :global(.item) + .empty { border-top: 1px solid var(--line); }
</style>
