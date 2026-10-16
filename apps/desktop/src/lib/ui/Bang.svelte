<script lang="ts">
  /**
   * The mark of a problem, the same on a page in the sidebar, a tab and a list entry: red while
   * dictation fails until it is fixed, orange while dictation goes on and it asks for attention.
   */
  let { title, tone = "live" }: { title: string; tone?: "live" | "warn" } = $props();
</script>

<span class="bang {tone}" role="img" aria-label={title} {title}>!</span>

<style>
  .bang { display: inline-block; flex: none; width: 16px; height: 16px; border-radius: 50%; background: var(--live); color: #fff; font: 700 11px/16px var(--font-round); text-align: center; vertical-align: middle; }
  .bang.warn { background: var(--warn); }
</style>
