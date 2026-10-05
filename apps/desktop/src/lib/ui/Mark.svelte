<script lang="ts">
  /** The brand mark: a ring with a gap at the upper right and a dot in the gap. */
  let { size = 26 }: { size?: number } = $props();
</script>

<svg class="mark" width={size} height={size} viewBox="12.5 12.5 75 75" aria-hidden="true">
  <!-- From the ring's lower right edge to its upper left edge. -->
  <defs>
    <linearGradient id="brand-mark" gradientUnits="userSpaceOnUse" x1="76.52" y1="76.52" x2="23.48" y2="23.48">
      <stop offset="0" stop-color="var(--brand-1)" /><stop offset="0.52" stop-color="var(--brand-2)" /><stop offset="1" stop-color="var(--brand-3)" />
    </linearGradient>
  </defs>
  <path d="M80.96 48.38A31 31 0 1 1 51.62 19.04" /><circle cx="71.92" cy="28.08" r="7.8" />
</svg>

<style>
  .mark { flex: none; }
  path { fill: none; stroke: url(#brand-mark); stroke-width: 13; stroke-linecap: round; }
  circle { fill: url(#brand-mark); }
</style>
