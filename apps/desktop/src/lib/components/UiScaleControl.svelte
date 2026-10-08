<script lang="ts">
  // Interface size (page zoom). A text-size icon with the current percentage
  // opens a small panel, so it reads as "make the app bigger" rather than as
  // another picture zoom.
  import { t } from "../i18n/index.svelte";
  import { app } from "../stores/app.svelte";
  import { layout, SCALES } from "../stores/layout.svelte";
  import Icon from "./Icon.svelte";

  let open = $state(false);
  let wrap: HTMLElement | undefined = $state();

  const percent = (scale: number) => Math.round(scale * 100);
  const mod = $derived(app.boot?.platform === "macos" ? "⌘" : "Ctrl");
  const index = $derived(SCALES.indexOf(layout.scale));

  // Escape closes the panel before anything else (the viewer would close too).
  function keydown(e: KeyboardEvent) {
    if (open && e.key === "Escape") {
      open = false;
      e.stopPropagation();
      e.preventDefault();
    }
  }

  function pointerdown(e: PointerEvent) {
    if (open && wrap && !wrap.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onkeydowncapture={keydown} onpointerdown={pointerdown} />

<div class="wrap" bind:this={wrap}>
  <button
    class="trigger"
    class:changed={layout.scale !== 1}
    aria-haspopup="dialog"
    aria-expanded={open}
    title={t("layout.scale_hint", { mod })}
    aria-label="{t('layout.scale')} {percent(layout.scale)}%"
    onclick={() => (open = !open)}
  >
    <span class="glyph"><Icon name="textsize" size={17} /></span>
    <span class="value">{percent(layout.scale)}%</span>
  </button>

  {#if open}
    <div class="popover" role="dialog" aria-label={t("layout.scale")}>
      <strong>{t("layout.scale")}</strong>
      <p class="muted">{t("layout.scale_intro")}</p>
      <div class="stepper">
        <button title={t("layout.scale_smaller")} aria-label={t("layout.scale_smaller")} disabled={index <= 0} onclick={() => layout.stepScale(-1)}>
          <span class="small-a">A</span>
        </button>
        <span class="current">{percent(layout.scale)}%</span>
        <button
          title={t("layout.scale_larger")}
          aria-label={t("layout.scale_larger")}
          disabled={index === SCALES.length - 1}
          onclick={() => layout.stepScale(1)}
        >
          <span class="big-a">A</span>
        </button>
      </div>
      <div class="presets" role="radiogroup" aria-label={t("layout.scale")}>
        {#each SCALES as scale (scale)}
          <button role="radio" aria-checked={layout.scale === scale} class:on={layout.scale === scale} onclick={() => layout.setScale(scale)}>
            {percent(scale)}%
          </button>
        {/each}
      </div>
      <p class="faint keys">{t("layout.scale_keys", { mod })}</p>
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }
  /* Styled like the language picker next to it: a filled, outlined control
     in the main text colour, distinct from the picture zoom's ghost buttons. */
  .trigger {
    gap: 5px;
    padding: 3px 8px 3px 6px;
    border: 1px solid var(--border-strong);
    background: var(--panel-2);
    color: var(--text);
  }
  .trigger[aria-expanded="true"] {
    border-color: var(--accent);
  }
  .glyph {
    display: inline-flex;
    color: var(--text);
  }
  .value {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .trigger.changed {
    border-color: color-mix(in srgb, var(--accent-2) 55%, var(--border-strong));
  }
  .trigger.changed .glyph {
    color: var(--accent-2);
  }
  .popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 40;
    width: 260px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    background: var(--panel);
    box-shadow: var(--shadow);
    cursor: default;
  }
  .popover p {
    margin: 0;
    font-size: 12px;
  }
  .stepper {
    display: grid;
    grid-template-columns: 44px 1fr 44px;
    align-items: center;
    gap: 8px;
  }
  .stepper button {
    justify-content: center;
    height: 36px;
    padding: 0;
  }
  .small-a {
    font-size: 12px;
    font-weight: 700;
  }
  .big-a {
    font-size: 20px;
    font-weight: 700;
  }
  .current {
    text-align: center;
    font-size: 20px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .presets button {
    border: none;
    border-radius: 0;
    padding: 5px 0;
    justify-content: center;
    background: var(--panel-2);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }
  .presets button + button {
    border-left: 1px solid var(--border);
  }
  .presets button.on {
    background: var(--accent-grad);
    color: #fff;
  }
  .keys {
    font-size: 11.5px;
  }
</style>
