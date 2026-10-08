<script lang="ts">
  // Before/after viewer. Both layers share one transform, so zoom and pan
  // stay aligned in every mode; the original is drawn at the result's size.
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { untrack } from "svelte";
  import { duration as fmtDuration, scaleText } from "../format";
  import { t } from "../i18n/index.svelte";
  import { ipc } from "../ipc";
  import { viewer, type CompareMode } from "../stores/viewer.svelte";
  import FilterPanel from "./FilterPanel.svelte";
  import Icon from "./Icon.svelte";
  import UiScaleControl from "./UiScaleControl.svelte";

  const MIN_ZOOM = 0.25;
  const MAX_ZOOM = 32;

  const item = $derived(viewer.current!);
  const isVideo = $derived(item.kind === "video");
  const originalSrc = $derived(convertFileSrc(item.original));
  const resultSrc = $derived(convertFileSrc(item.result));

  let paneW = $state(0);
  let paneH = $state(0);
  let natural = $state<[number, number] | null>(null);
  let zoom = $state(1);
  let x = $state(0);
  let y = $state(0);
  let split = $state(0.5);
  let mix = $state(1);

  // Fitted box of the content inside one pane.
  const aspect = $derived.by(() => {
    const size = item.resultSize ?? natural;
    return size && size[1] > 0 ? size[0] / size[1] : 16 / 9;
  });
  const fitW = $derived(paneW / paneH > aspect ? paneH * aspect : paneW);
  const fitH = $derived(fitW / aspect);
  const left = $derived((paneW - fitW) / 2);
  const top = $derived((paneH - fitH) / 2);
  const dpr = typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
  const resultW = $derived(item.resultSize?.[0] ?? natural?.[0] ?? fitW);
  const sourceW = $derived(item.sourceSize?.[0] ?? resultW);
  // Show real pixels once an image is magnified past 1:1 on screen.
  const actual = $derived(resultW / dpr / fitW);

  // The two image layers: the overlay (left) and the base (right). While the
  // filter panel applies filters, the filtered picture takes the right and
  // the left shows what it is compared against. Labels on the picture, the
  // panel's summary and the fade buttons all read from here.
  type Layer = { src: string; width: number; label: string };
  const filterLayers = $derived.by((): [Layer, Layer] | null => {
    if (!viewer.filtering) return null;
    const original: Layer = { src: originalSrc, width: sourceW, label: t("viewer.original") };
    const result: Layer = { src: resultSrc, width: resultW, label: t("viewer.result") };
    const [target, other] = viewer.target === "result" ? [result, original] : [original, result];
    const after: Layer = {
      src: viewer.processed ? convertFileSrc(viewer.processed) : target.src,
      width: target.width,
      label: t("viewer.filters_with", { name: target.label }),
    };
    return [viewer.base === "unfiltered" ? target : other, after];
  });
  const layers = $derived.by((): [Layer, Layer] => {
    const plain: [Layer, Layer] = [
      { src: originalSrc, width: sourceW, label: t("viewer.original") },
      { src: resultSrc, width: resultW, label: t("viewer.result") },
    ];
    // Until the first preview arrives, show (and label) the unfiltered pair.
    return filterLayers && viewer.processed ? filterLayers : plain;
  });
  const sides = $derived((filterLayers ?? layers).map((l) => l.label) as [string, string]);

  type Badge = { tone: "on" | "off" | "busy" | "err"; text: string };
  const badge = $derived.by((): Badge | null => {
    if (!viewer.panel || isVideo) return null;
    if (!viewer.applied) return { tone: "off", text: t("viewer.filters_state_off") };
    if (viewer.error) return { tone: "err", text: t("viewer.filters_state_failed") };
    if (viewer.activeSteps === 0) return { tone: "off", text: t("viewer.filters_state_empty") };
    if (viewer.busy || !viewer.processed) return { tone: "busy", text: t("viewer.filters_state_busy") };
    return { tone: "on", text: t("viewer.filters_state_on", { count: viewer.activeSteps }) };
  });

  const transform = $derived(`translate(${left + x}px, ${top + y}px) scale(${zoom})`);

  // Keep the same content point centred when the pane resizes (mode switch,
  // window resize), so switching modes never loses the area being inspected.
  let lastGeom: { w: number; h: number; left: number; top: number; fw: number; fh: number } | null = null;
  $effect(() => {
    const g = { w: paneW, h: paneH, left, top, fw: fitW, fh: fitH };
    untrack(() => {
      const o = lastGeom;
      if (o && o.fw > 0 && o.fh > 0 && g.fw > 0 && (o.w !== g.w || o.h !== g.h)) {
        const u = (o.w / 2 - o.left - x) / (zoom * o.fw);
        const v = (o.h / 2 - o.top - y) / (zoom * o.fh);
        x = g.w / 2 - g.left - u * zoom * g.fw;
        y = g.h / 2 - g.top - v * zoom * g.fh;
      }
      lastGeom = g;
    });
  });

  // Reset the view when another item opens.
  $effect(() => {
    void item.result;
    zoom = 1;
    x = 0;
    y = 0;
    natural = null;
    playing = false;
    time = 0;
  });

  function zoomAt(cx: number, cy: number, next: number) {
    next = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next));
    const px = (cx - left - x) / zoom;
    const py = (cy - top - y) / zoom;
    x = cx - left - px * next;
    y = cy - top - py * next;
    zoom = next;
  }

  function fit() {
    zoom = 1;
    x = 0;
    y = 0;
  }

  function wheel(e: WheelEvent) {
    e.preventDefault();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const factor = Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0025));
    zoomAt(e.clientX - rect.left, e.clientY - rect.top, zoom * factor);
  }

  let drag: { kind: "pan" | "split"; startX: number; startY: number; x: number; y: number; width: number } | null = null;

  function down(e: PointerEvent, kind: "pan" | "split") {
    if (e.button !== 0) return;
    e.stopPropagation();
    const pane = (e.currentTarget as HTMLElement).closest(".pane") as HTMLElement;
    pane.setPointerCapture(e.pointerId);
    drag = { kind, startX: e.clientX, startY: e.clientY, x, y, width: pane.getBoundingClientRect().width };
    if (kind === "split") move(e);
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    if (drag.kind === "pan") {
      x = drag.x + e.clientX - drag.startX;
      y = drag.y + e.clientY - drag.startY;
    } else {
      const pane = (e.currentTarget as HTMLElement).closest(".pane") as HTMLElement;
      const rect = pane.getBoundingClientRect();
      split = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    }
  }

  function up() {
    drag = null;
  }

  // ---- video sync -----------------------------------------------------------
  let originalVideo: HTMLVideoElement | undefined = $state();
  let resultVideo: HTMLVideoElement | undefined = $state();
  let playing = $state(false);
  let time = $state(0);
  let length = $state(0);

  function sync() {
    if (!originalVideo || !resultVideo) return;
    if (Math.abs(originalVideo.currentTime - resultVideo.currentTime) > 0.08) {
      originalVideo.currentTime = resultVideo.currentTime;
    }
  }

  $effect(() => {
    if (!playing || !resultVideo) return;
    let frame = 0;
    const tick = () => {
      time = resultVideo?.currentTime ?? 0;
      sync();
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  });

  async function togglePlay() {
    if (!resultVideo) return;
    if (resultVideo.paused) {
      sync();
      await Promise.all([resultVideo.play(), originalVideo?.play()]);
      playing = true;
    } else {
      resultVideo.pause();
      originalVideo?.pause();
      playing = false;
    }
  }

  function seek(value: number) {
    time = value;
    if (resultVideo) resultVideo.currentTime = value;
    if (originalVideo) originalVideo.currentTime = value;
  }

  // Mode switches recreate the video elements; carry the position over.
  function restore(video: HTMLVideoElement) {
    if (Math.abs(video.currentTime - time) > 0.05) video.currentTime = time;
    if (playing) void video.play();
  }

  function setMode(mode: CompareMode) {
    viewer.mode = mode;
  }

  function key(e: KeyboardEvent) {
    if (["INPUT", "SELECT", "TEXTAREA"].includes((e.target as HTMLElement)?.tagName)) return;
    // Cmd/Ctrl chords belong to the app (content scale), not the picture.
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key;
    if (k === "Escape") viewer.close();
    else if (k === "f" || k === "F") viewer.panel = !viewer.panel;
    else if (k === "\\" && !isVideo) viewer.toggleApplied();
    else if (k === "1") setMode("slider");
    else if (k === "2") setMode("split");
    else if (k === "3") setMode("fade");
    else if (k === "+" || k === "=") zoomAt(paneW / 2, paneH / 2, zoom * 1.25);
    else if (k === "-") zoomAt(paneW / 2, paneH / 2, zoom / 1.25);
    else if (k === "0") fit();
    else if (k === "ArrowLeft") viewer.step(-1);
    else if (k === "ArrowRight") viewer.step(1);
    else if (k === " " && isVideo) togglePlay();
    else return;
    e.preventDefault();
  }

  const modes: { id: CompareMode; icon: string; key: "viewer.mode_slider" | "viewer.mode_split" | "viewer.mode_fade" }[] = [
    { id: "slider", icon: "slider", key: "viewer.mode_slider" },
    { id: "split", icon: "split", key: "viewer.mode_split" },
    { id: "fade", icon: "fade", key: "viewer.mode_fade" },
  ];

  function onResultLoad(e: Event) {
    const el = e.currentTarget as HTMLImageElement | HTMLVideoElement;
    if (el instanceof HTMLImageElement) natural = [el.naturalWidth, el.naturalHeight];
    else {
      natural = [el.videoWidth, el.videoHeight];
      length = el.duration;
      restore(el);
    }
  }
</script>

<svelte:window onkeydown={key} />

{#snippet layer(index: 0 | 1)}
  {@const shown = layers[index]}
  <div class="content" style:width="{fitW}px" style:height="{fitH}px" style:transform={transform}>
    {#if isVideo}
      {#if index === 0}
        <video bind:this={originalVideo} src={originalSrc} muted playsinline preload="auto" onloadedmetadata={(e) => restore(e.currentTarget)}></video>
      {:else}
        <!-- Upscaled copy of the user's own video: captions, if any, live in the source file. -->
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={resultVideo} src={resultSrc} playsinline preload="auto" onloadedmetadata={onResultLoad} onended={() => (playing = false)}></video>
      {/if}
    {:else}
      <img
        src={shown.src}
        alt={shown.label}
        draggable="false"
        class:pixel={(fitW * zoom * dpr) / shown.width > 1.5}
        onload={shown.src === resultSrc ? onResultLoad : undefined}
      />
    {/if}
  </div>
{/snippet}

<div class="viewer" role="dialog" aria-modal="true" aria-label={t("viewer.title")}>
  <header>
    <div class="info">
      <strong title={item.result}>{item.name}</strong>
      <span class="muted">
        {#if item.sourceSize}{item.sourceSize[0]}×{item.sourceSize[1]}{/if}
        {#if item.resultSize}<span class="arrow">→</span>{item.resultSize[0]}×{item.resultSize[1]}{/if}
        {#if item.model}· {item.model}{/if}
        {#if item.scale}· ×{scaleText(item.scale)}{/if}
        {#if item.elapsedMs}· {fmtDuration(item.elapsedMs)}{/if}
      </span>
    </div>

    <div class="segmented" role="radiogroup" aria-label={t("viewer.mode")}>
      {#each modes as m, i (m.id)}
        <button
          role="radio"
          aria-checked={viewer.mode === m.id}
          class:on={viewer.mode === m.id}
          title="{t(m.key)} ({i + 1})"
          onclick={() => setMode(m.id)}
        >
          <Icon name={m.icon} size={14} />
          <span class="collapse">{t(m.key)}</span>
        </button>
      {/each}
    </div>

    <div class="tools">
      <button class="ghost" title={t("viewer.zoom_out")} onclick={() => zoomAt(paneW / 2, paneH / 2, zoom / 1.25)}><Icon name="minus" /></button>
      <span class="zoom" title={t("viewer.zoom_level")}>{Math.round(zoom * 100)}%</span>
      <button class="ghost" title={t("viewer.zoom_in")} onclick={() => zoomAt(paneW / 2, paneH / 2, zoom * 1.25)}><Icon name="plus" /></button>
      <button class="ghost" title={t("viewer.fit")} onclick={fit}><Icon name="fit" /></button>
      <button class="ghost text" title={t("viewer.actual_hint")} onclick={() => zoomAt(paneW / 2, paneH / 2, actual)}>1:1</button>
      <span class="sep"></span>
      {#if viewer.list.length > 1}
        <button class="ghost" title={t("viewer.prev")} onclick={() => viewer.step(-1)}><Icon name="left" /></button>
        <span class="faint count">{viewer.index + 1}/{viewer.list.length}</span>
        <button class="ghost" title={t("viewer.next")} onclick={() => viewer.step(1)}><Icon name="right" /></button>
      {/if}
      <button
        class="ghost filters-toggle"
        class:on={viewer.panel}
        class:applied={viewer.filtering}
        aria-pressed={viewer.panel}
        title={t("viewer.filters_hint")}
        onclick={() => (viewer.panel = !viewer.panel)}
      >
        <Icon name="sparkle" /> <span class="collapse">{t("viewer.filters")}</span>
        {#if viewer.filtering}<span class="dot" aria-hidden="true"></span>{/if}
      </button>
      <button class="ghost" title={t("queue.reveal")} onclick={() => ipc.reveal(item.result)}><Icon name="reveal" /></button>
      <span class="sep"></span>
      <UiScaleControl />
      <button class="ghost" title={t("common.close")} onclick={() => viewer.close()}><Icon name="x" /></button>
    </div>
  </header>

  <div class="body">
  <div class="stage" class:split={viewer.mode === "split"}>
    {#if viewer.mode === "split"}
      {#each [0, 1] as const as which (which)}
        <div
          class="pane"
          role="presentation"
          bind:clientWidth={paneW}
          bind:clientHeight={paneH}
          onwheel={wheel}
          onpointerdown={(e) => down(e, "pan")}
          onpointermove={move}
          onpointerup={up}
          onpointercancel={up}
        >
          {@render layer(which)}
          <span class="label {which === 0 ? 'left' : 'right'}">{layers[which].label}</span>
        </div>
      {/each}
    {:else}
      <div
        class="pane"
        role="presentation"
        bind:clientWidth={paneW}
        bind:clientHeight={paneH}
        onwheel={wheel}
        onpointerdown={(e) => down(e, "pan")}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={up}
      >
        {@render layer(1)}
        <div
          class="overlay"
          style:clip-path={viewer.mode === "slider" ? `inset(0 ${(1 - split) * 100}% 0 0)` : "none"}
          style:opacity={viewer.mode === "fade" ? 1 - mix : 1}
        >
          {@render layer(0)}
        </div>
        {#if viewer.mode === "slider"}
          <div class="divider" style:left="{split * 100}%">
            <button
              class="knob"
              aria-label={t("viewer.drag_divider")}
              onpointerdown={(e) => down(e, "split")}
            ><Icon name="slider" size={14} /></button>
          </div>
          <span class="label left">{layers[0].label}</span>
          <span class="label right">{layers[1].label}</span>
        {/if}
      </div>
    {/if}
  </div>
  {#if badge}
    <button
      class="badge {badge.tone}"
      title={t("viewer.filters_apply_hint")}
      aria-pressed={viewer.applied}
      onclick={() => (viewer.applied = !viewer.applied)}
    >
      {#if badge.tone === "busy"}<span class="spinner" aria-hidden="true"></span>{:else}<span class="led" aria-hidden="true"></span>{/if}
      {badge.text}
    </button>
  {/if}
  {#if viewer.panel}
    <FilterPanel {item} {sides} />
  {/if}
  </div>

  <footer>
    {#if viewer.mode === "fade"}
      <div class="fade">
        <button class="ghost text" onclick={() => (mix = 0)}>{layers[0].label}</button>
        <input type="range" min="0" max="1" step="0.01" bind:value={mix} aria-label={t("viewer.mix")} />
        <button class="ghost text" onclick={() => (mix = 1)}>{layers[1].label}</button>
      </div>
    {/if}
    {#if isVideo}
      <div class="player">
        <button class="ghost" title={playing ? t("viewer.pause") : t("viewer.play")} onclick={togglePlay}>
          <Icon name={playing ? "pause" : "play"} />
        </button>
        <input
          type="range"
          min="0"
          max={length || 0}
          step="0.01"
          value={time}
          oninput={(e) => seek((e.currentTarget as HTMLInputElement).valueAsNumber)}
          aria-label={t("viewer.seek")}
        />
        <span class="faint time">{time.toFixed(1)} / {length.toFixed(1)}s</span>
      </div>
    {/if}
    <span class="faint hint">{t("viewer.hint")}</span>
  </footer>
</div>

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    animation: fade 120ms ease-out;
  }
  header {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }
  .info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .info strong,
  .info span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .info span {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .arrow {
    margin: 0 4px;
  }
  .segmented {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .segmented button {
    border: none;
    border-radius: 0;
    background: var(--panel-2);
  }
  .segmented button + button {
    border-left: 1px solid var(--border);
  }
  .segmented button.on {
    background: var(--accent-grad);
    color: #fff;
  }
  .tools {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
  }
  .zoom,
  .count {
    min-width: 44px;
    text-align: center;
    font-variant-numeric: tabular-nums;
    font-size: 12px;
  }
  .text {
    font-size: 12px;
    font-weight: 600;
  }
  .tools .on {
    color: var(--accent);
  }
  /* Narrow windows (or a large content scale): two header rows, icon-only modes. */
  @media (max-width: 760px) {
    header {
      grid-template-columns: 1fr auto;
      row-gap: 6px;
    }
    .info {
      grid-column: 1 / -1;
    }
    .segmented {
      justify-self: start;
    }
    .collapse {
      display: none;
    }
    .zoom,
    .count {
      min-width: 36px;
    }
    .hint {
      display: none;
    }
  }
  .sep {
    width: 1px;
    height: 18px;
    background: var(--border);
    margin: 0 6px;
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .filters-toggle {
    position: relative;
  }
  .filters-toggle .dot {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent-2);
    box-shadow: 0 0 0 2px var(--panel);
  }
  .badge {
    position: absolute;
    top: 12px;
    left: 12px;
    z-index: 4;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 4px 11px;
    border-radius: 99px;
    border: 1px solid transparent;
    font-size: 12px;
    font-weight: 600;
    color: #fff;
    background: rgb(0 0 0 / 0.6);
    backdrop-filter: blur(6px);
  }
  .badge.on {
    background: color-mix(in srgb, var(--accent) 85%, black);
    border-color: color-mix(in srgb, var(--accent-2) 60%, transparent);
  }
  .badge.off {
    color: #d5d8e2;
  }
  .badge.err {
    background: color-mix(in srgb, var(--err) 75%, black);
  }
  .led {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #8a92a8;
  }
  .badge.on .led {
    background: var(--accent-2);
    box-shadow: 0 0 6px var(--accent-2);
  }
  .badge.err .led {
    background: #fff;
  }
  .spinner {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 2px solid rgb(255 255 255 / 0.35);
    border-top-color: #fff;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .stage {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr;
    background: repeating-conic-gradient(var(--panel-2) 0 25%, var(--panel-3) 0 50%) 0 0 / 20px 20px;
  }
  .stage.split {
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    background-color: var(--border);
  }
  .pane {
    position: relative;
    overflow: hidden;
    cursor: grab;
    touch-action: none;
    background: repeating-conic-gradient(var(--panel-2) 0 25%, var(--panel-3) 0 50%) 0 0 / 20px 20px;
  }
  .pane:active {
    cursor: grabbing;
  }
  .content {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    will-change: transform;
  }
  .content img,
  .content video {
    width: 100%;
    height: 100%;
    display: block;
    object-fit: fill;
    pointer-events: none;
    user-select: none;
  }
  .content img.pixel {
    image-rendering: pixelated;
  }
  .overlay {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .divider {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: #fff;
    box-shadow: 0 0 8px rgb(0 0 0 / 0.5);
  }
  .knob {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 32px;
    height: 32px;
    padding: 0;
    border-radius: 50%;
    border: none;
    background: #fff;
    color: #111;
    justify-content: center;
    cursor: ew-resize;
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.4);
  }
  .label {
    position: absolute;
    bottom: 12px;
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 99px;
    background: rgb(0 0 0 / 0.55);
    color: #fff;
    pointer-events: none;
  }
  .left {
    left: 12px;
  }
  .right {
    right: 12px;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 8px 14px;
    border-top: 1px solid var(--border);
    background: var(--panel);
    min-height: 44px;
  }
  .fade,
  .player {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    max-width: 520px;
  }
  .time {
    font-variant-numeric: tabular-nums;
    font-size: 12px;
    white-space: nowrap;
  }
  .hint {
    margin-left: auto;
    font-size: 11.5px;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
