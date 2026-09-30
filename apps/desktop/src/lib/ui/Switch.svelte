<script lang="ts">
  let { checked = $bindable(false), label, disabled = false, onchange }: {
    checked?: boolean;
    /** Accessible name when no visible label is associated. */
    label?: string;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  } = $props();

  function toggle() {
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button type="button" role="switch" class="switch" class:on={checked} aria-checked={checked} aria-label={label} {disabled} onclick={toggle}></button>

<style>
  .switch { width: 34px; height: 20px; padding: 0; border: 0; border-radius: 10px; background: var(--line-strong); position: relative; flex: none; cursor: default; }
  .switch::after { content: ""; position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: var(--knob); box-shadow: var(--knob-shadow); transition: transform .15s; }
  .switch.on { background: var(--accent); }
  .switch.on::after { transform: translateX(14px); }
  .switch:disabled { opacity: .5; }
</style>
