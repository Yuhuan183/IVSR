<script lang="ts">
  // Width changes are applied as a transform so updates stay on the compositor.
  let { value, tone = "accent" }: { value: number; tone?: "accent" | "ok" | "err" | "muted" } = $props();
</script>

<div class="track">
  <div class="fill {tone}" style:transform="scaleX({Math.max(0, Math.min(1, value))})"></div>
</div>

<style>
  .track {
    height: 4px;
    border-radius: 4px;
    background: var(--panel-3);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    width: 100%;
    transform-origin: left center;
    transition: transform 140ms linear;
    border-radius: 4px;
  }
  .accent {
    background: var(--accent-grad);
  }
  .ok {
    background: var(--ok);
  }
  .err {
    background: var(--err);
  }
  .muted {
    background: var(--faint);
  }
</style>
