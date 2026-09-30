<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, reveal, systemInfo, type SystemInfo } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { toast } from "../../lib/ui/toast.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Icon from "../../lib/ui/Icon.svelte";

  let info = $state<SystemInfo | null>(null);
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
  const runtimes = ["whisper.cpp", "llama.cpp", "ggml"];

  onMount(async () => {
    try { info = await systemInfo(); } catch (e) { toast.error(errorMessage(e)); }
  });

  async function show(path: string) {
    try { await reveal(path); } catch (e) { toast.error(errorMessage(e)); }
  }
</script>

<PageHead title={t("nav.about")} />

<div class="app">
  <span class="mark" aria-hidden="true"></span>
  <div>
    <div class="app-name">{t("app.title")}</div>
    <div class="sub">{info ? t("about.version", { version: info.version }) : ""}</div>
  </div>
</div>
<p class="license">{t("about.license")}</p>

{#if info}
  <Group title={t("about.locations")}>
    {#each [["about.config_dir", info.config_dir], ["about.data_dir", info.data_dir], ["about.cache_dir", info.cache_dir]] as [key, path] (key)}
      <Row label={t(key)} help={path}>
        <button type="button" class="btn quiet" onclick={() => show(path)}><Icon name="folder" size={14} />{t("about.reveal")}</button>
      </Row>
    {/each}
  </Group>
{/if}

<Group title={t("about.licenses")} help={t("about.model_licenses")}>
  <details>
    <summary><span class="name">{runtimes.join(", ")}</span><span class="sub">MIT</span><span class="chev"><Icon name="chevron-right" size={14} /></span></summary>
    <pre class="notice">{noticeText}</pre>
  </details>
</Group>

<style>
  .app { display: flex; align-items: center; gap: 14px; }
  .mark { width: 40px; height: 40px; border-radius: 50%; border: 6px solid var(--accent); flex: none; }
  .app-name { font: 600 14px/1.3 var(--font-round); }
  .license { margin: 12px 0 0; color: var(--ink-2); }
  details summary { display: flex; align-items: center; gap: 10px; min-height: 44px; padding: 8px 14px; list-style: none; cursor: default; border-radius: 9px; }
  details summary::-webkit-details-marker { display: none; }
  details summary:hover { background: var(--sunken); }
  details summary:focus-visible { outline-offset: -2px; }
  summary .name { flex: 1; min-width: 0; }
  .chev { display: flex; color: var(--ink-3); transition: transform .15s; }
  details[open] .chev { transform: rotate(90deg); }
  details[open] summary { border-radius: 9px 9px 0 0; }
  .notice { margin: 0; padding: 12px 14px 14px; border-top: 1px solid var(--line); white-space: pre-wrap; font: 11px/1.55 var(--font-mono); color: var(--ink-2); max-height: 260px; overflow: auto; }
</style>
