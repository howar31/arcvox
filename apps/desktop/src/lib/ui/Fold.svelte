<script lang="ts">
  /** Content that appears or goes with a setting: it slides open and shut instead of jumping. */
  import type { Snippet } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { slide } from "svelte/transition";
  let { children, onoutroend }: { children: Snippet; /** Called once the content has slid shut. */ onoutroend?: () => void } = $props();
</script>

<div class="fold" transition:slide={{ duration: 220, easing: cubicOut }} {onoutroend}>{@render children()}</div>

<style>
  .fold { overflow: hidden; }
</style>
