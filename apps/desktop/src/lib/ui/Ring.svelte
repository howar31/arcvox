<script lang="ts">
  /** The app's mark, used as the only status glyph. */
  import { tip } from "../tip";
  let { tone = "ok", size = "md", title }: { tone?: "ok" | "accent" | "live" | "warn" | "off"; size?: "sm" | "md"; /** What the colour means, shown while the pointer rests on it. */ title?: string } = $props();
</script>

<span class="ring {tone} {size}" aria-hidden={title ? undefined : true} role={title ? "img" : undefined} aria-label={title} use:tip={title}></span>

<style>
  .ring { display: inline-block; width: 14px; height: 14px; border-radius: 50%; border: 2.5px solid var(--ok); flex: none; }
  .ring.live { border-color: var(--live); }
  .ring.warn { border-color: var(--warn); }
  .ring.off { border-color: var(--ink-3); }
  .ring.accent { border-color: var(--accent); }
  .ring.sm { width: 10px; height: 10px; border-width: 2px; }
</style>
