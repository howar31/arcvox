<script lang="ts">
  /** The app's mark, used as the only status glyph. */
  let { tone = "ok", size = "md" }: { tone?: "ok" | "accent" | "live" | "off"; size?: "sm" | "md" } = $props();
</script>

<span class="ring {tone} {size}" aria-hidden="true"></span>

<style>
  .ring { display: inline-block; width: 14px; height: 14px; border-radius: 50%; border: 2.5px solid var(--ok); flex: none; }
  .ring.live { border-color: var(--live); }
  .ring.off { border-color: var(--ink-3); }
  .ring.accent { border-color: var(--accent); }
  .ring.sm { width: 10px; height: 10px; border-width: 2px; }
</style>
