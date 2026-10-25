<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, onUpdateStatus, openLink, updateReleases, type Release } from "../../lib/api";
  import { currentLanguage, t } from "../../lib/i18n";
  import { sections, title } from "../../lib/whatsNew";
  import { toast } from "../../lib/ui/toast.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";

  let releases = $state<Release[]>([]);
  const list = $derived(sections(releases, currentLanguage()));
  const heading = $derived(title(list, t) || t("about.update.whats_new"));

  onMount(() => {
    const load = () => updateReleases().then((r) => { releases = r; }).catch(() => {});
    load();
    // The releases are read before a state is announced.
    const off = onUpdateStatus(() => load());
    return () => { off.then((f) => f()); };
  });

  async function all() {
    try { await openLink("releases"); } catch (e) { toast.error(errorMessage(e)); }
  }
</script>

<PageHead title={heading} crumb={{ label: t("nav.about"), page: "about" }} />

<!-- One release needs no version heading: the page title names it. -->
{#each list as s (s.version)}
  <Group title={list.length > 1 ? s.version : undefined}>
    {#each s.notes as n, i (i)}
      <Row label={n.lead || n.detail} help={n.lead ? n.detail : undefined} />
    {/each}
  </Group>
{/each}

<Group>
  <Row label={t("about.update.all_releases")} onclick={all} external />
</Group>
