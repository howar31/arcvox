<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, runningApps, type RunningApp } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import Row from "../../lib/ui/Row.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  /**
   * `value` is the bundle id chosen or typed but not added yet; the page binds it so that text counts
   * as an unsaved change and its save can add it first.
   */
  let { onadd, value = $bindable("") }: { onadd: (id: string) => void; value?: string } = $props();
  let apps = $state<RunningApp[]>([]);
  onMount(async () => {
    try { apps = await runningApps(); } catch (e) { toast.error(errorMessage(e)); }
  });

  function add() {
    const id = value.trim();
    if (!id) return;
    onadd(id);
    value = "";
  }
</script>

<Row>
  <select class="apps" bind:value aria-label={t("general.pick_app")}>
    <option value="">{t("general.pick_app")}</option>
    {#each apps as a (a.bundle_id)}<option value={a.bundle_id}>{a.name}</option>{/each}
  </select>
  <input class="add-input" type="text" placeholder="com.example.app" aria-label={t("general.bundle_id")} bind:value onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) add(); }} />
  <button type="button" class="btn" disabled={!value.trim()} onclick={add}><Icon name="plus" size={14} />{t("common.add")}</button>
</Row>

<style>
  /* The picker gives way first, so the text box keeps the width of the other add rows. */
  .apps { width: 170px; flex: 0 4 auto; min-width: 0; }
  .add-input { margin-left: auto; }
</style>
