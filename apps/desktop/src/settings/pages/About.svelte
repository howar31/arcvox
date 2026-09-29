<script lang="ts">
  import { onMount } from "svelte";
  import { reveal, systemInfo, type SystemInfo } from "../../lib/api";
  import { t } from "../../lib/i18n";
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
  onMount(async () => { info = await systemInfo(); });
</script>
<h2>{t("nav.about")}</h2>
{#if info}
  <p>Arcvox {info.version}</p>
  <p>{t("about.license")}</p>
  <dl>
    <dt>{t("about.config_dir")}</dt><dd><code>{info.config_dir}</code> <button onclick={() => reveal(info!.config_dir)}>{t("about.reveal")}</button></dd>
    <dt>{t("about.data_dir")}</dt><dd><code>{info.data_dir}</code> <button onclick={() => reveal(info!.data_dir)}>{t("about.reveal")}</button></dd>
    <dt>{t("about.cache_dir")}</dt><dd><code>{info.cache_dir}</code> <button onclick={() => reveal(info!.cache_dir)}>{t("about.reveal")}</button></dd>
  </dl>
{/if}
<h3>{t("about.runtime_licenses")}</h3>
<ul>
  <li>whisper.cpp (MIT)</li>
  <li>llama.cpp (MIT)</li>
  <li>ggml (MIT)</li>
</ul>
<pre class="notice">{mitNotice}</pre>
<p>{t("about.model_licenses")}</p>
<style>
  .notice { white-space: pre-wrap; font-size: 0.8em; }
</style>
