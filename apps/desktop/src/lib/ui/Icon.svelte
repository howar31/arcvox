<script lang="ts" module>
  /** 16 x 16 outline icons drawn with the current text color. */
  const PATHS = {
    home: '<path d="M3 13V8M6.5 13V3.5M10 13V9.5M13.5 13V6"/>',
    flow: '<rect x="1.5" y="5.5" width="3.4" height="5" rx="1"/><rect x="6.3" y="5.5" width="3.4" height="5" rx="1"/><rect x="11.1" y="5.5" width="3.4" height="5" rx="1"/>',
    profiles: '<rect x="2.5" y="3" width="11" height="10" rx="2"/><path d="M5.5 6.5h5M5.5 9.5h3"/>',
    engines: '<rect x="3" y="3" width="10" height="10" rx="2"/><path d="M6 1.5v1.5M10 1.5v1.5M6 13v1.5M10 13v1.5M1.5 6h1.5M1.5 10h1.5M13 6h1.5M13 10h1.5"/>',
    dictionary: '<path d="M3.5 2.5h7a2 2 0 0 1 2 2v9h-7a2 2 0 0 1-2-2z"/><path d="M6 6h4M6 8.5h2.5"/>',
    prompt: '<path d="M3 4h10M3 8h10M3 12h6"/>',
    history: '<circle cx="8" cy="8" r="5.5"/><path d="M8 5v3l2 1.5"/>',
    general: '<path d="M2.5 5h4M10.5 5h3M2.5 11h2M8.5 11h5"/><circle cx="8.5" cy="5" r="1.7"/><circle cx="6.5" cy="11" r="1.7"/>',
    play: '<path d="M5 3.2v9.6L12.5 8z"/>',
    pause: '<path d="M5.5 3.5v9M10.5 3.5v9"/>',
    search: '<circle cx="7" cy="7" r="4.2"/><path d="M10.2 10.2 13.5 13.5"/>',
    speed: '<path d="M2.5 11a5.5 5.5 0 1 1 11 0"/><path d="M8 11l2.5-3.5"/>',
    about: '<circle cx="8" cy="8" r="5.5"/><path d="M8 7.5V11M8 5.2v.1"/>',
    plus: '<path d="M8 3v10M3 8h10"/>',
    check: '<path d="M3.5 8.5l3 3 6-7"/>',
    x: '<path d="M4 4l8 8M12 4l-8 8"/>',
    "chevron-right": '<path d="M6 3.5 10.5 8 6 12.5"/>',
    "chevron-left": '<path d="M10 3.5 5.5 8l4.5 4.5"/>',
    "chevron-down": '<path d="M3.5 6 8 10.5 12.5 6"/>',
    copy: '<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 3.5v-.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h.5"/>',
    trash: '<path d="M2.5 4.5h11M6.5 4.5V3a.5.5 0 0 1 .5-.5h2a.5.5 0 0 1 .5.5v1.5M4 4.5l.7 8.2a1 1 0 0 0 1 .8h4.6a1 1 0 0 0 1-.8l.7-8.2"/>',
    "arrow-up": '<path d="M8 13V3M4 7l4-4 4 4"/>',
    "arrow-down": '<path d="M8 3v10M4 9l4 4 4-4"/>',
    mic: '<rect x="6" y="2" width="4" height="7.5" rx="2"/><path d="M3.5 7.5a4.5 4.5 0 0 0 9 0M8 12v2"/>',
    download: '<path d="M8 2.5v8M4.5 7 8 10.5 11.5 7M3 13.5h10"/>',
    folder: '<path d="M2 4.5a1 1 0 0 1 1-1h3.2l1.5 1.5H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/>',
    warning: '<path d="M8 2.5 14 13H2z"/><path d="M8 6.5v3M8 11.3v.1"/>',
    refresh: '<path d="M13 8a5 5 0 1 1-1.5-3.55"/><path d="M13 2.5v2.5h-2.5"/>',
  } as const;
  export type IconName = keyof typeof PATHS;
</script>

<script lang="ts">
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<svg class="icon" viewBox="0 0 16 16" width={size} height={size} aria-hidden="true">{@html PATHS[name]}</svg>

<style>
  .icon { flex: none; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
</style>
