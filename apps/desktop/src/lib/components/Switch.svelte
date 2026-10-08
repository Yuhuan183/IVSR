<script lang="ts">
  // An on/off switch whose state is visible at a glance (colour, knob and text).
  let {
    checked,
    onchange,
    label,
    title,
  }: { checked: boolean; onchange: (on: boolean) => void; label: string; title?: string } = $props();
</script>

<button class="switch" class:on={checked} role="switch" aria-checked={checked} {title} onclick={() => onchange(!checked)}>
  <span class="track"><span class="knob"></span></span>
  <span class="text">{label}</span>
</button>

<style>
  .switch {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 2px 4px;
    border: none;
    background: transparent;
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
  }
  .switch.on {
    color: var(--text);
  }
  .track {
    position: relative;
    width: 30px;
    height: 17px;
    border-radius: 99px;
    background: var(--border-strong);
    transition: background 120ms;
    flex-shrink: 0;
  }
  .on .track {
    background: var(--accent-grad);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.35);
    transition: transform 120ms;
  }
  .on .knob {
    transform: translateX(13px);
  }
</style>
