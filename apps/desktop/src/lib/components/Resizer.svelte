<script lang="ts">
  // A vertical drag handle on a panel's left edge. Dragging left widens the
  // panel; `oncommit` fires once on release so callers persist only then.
  // Arrow keys nudge it; a double click restores `initial`.
  import { t } from "../i18n/index.svelte";

  let {
    width,
    min,
    max,
    initial,
    onresize,
    oncommit,
  }: {
    width: number;
    min: number;
    max: number;
    initial: number;
    onresize: (width: number) => void;
    oncommit: (width: number) => void;
  } = $props();

  let drag: { x: number; width: number } | null = null;
  const clamp = (w: number) => Math.round(Math.min(max, Math.max(min, w)));

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { x: e.clientX, width };
    e.preventDefault();
  }

  function move(e: PointerEvent) {
    if (drag) onresize(clamp(drag.width + drag.x - e.clientX));
  }

  // The release position counts too: some input sources skip the moves.
  function up(e: PointerEvent) {
    if (!drag) return;
    move(e);
    drag = null;
    oncommit(width);
  }

  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 40 : 10;
    if (e.key === "ArrowLeft") onresize(clamp(width + step));
    else if (e.key === "ArrowRight") onresize(clamp(width - step));
    else return;
    e.preventDefault();
    oncommit(clamp(width));
  }
</script>

<!-- A focusable separator is the WAI-ARIA window splitter pattern. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="resizer"
  role="separator"
  aria-orientation="vertical"
  aria-label={t("layout.resize")}
  aria-valuenow={width}
  aria-valuemin={min}
  aria-valuemax={max}
  tabindex="0"
  title={t("layout.resize")}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onkeydown={key}
  ondblclick={() => {
    onresize(clamp(initial));
    oncommit(clamp(initial));
  }}
></div>

<style>
  .resizer {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -3px;
    width: 7px;
    cursor: col-resize;
    z-index: 5;
    touch-action: none;
  }
  .resizer::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 1px;
    background: transparent;
    transition: background 120ms;
  }
  .resizer:hover::after,
  .resizer:focus-visible::after {
    background: var(--accent);
    width: 2px;
    left: 2px;
  }
  .resizer:focus-visible {
    outline: none;
  }
</style>
