<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { bytes, duration } from "../format";
  import { t, type MessageKey } from "../i18n/index.svelte";
  import { ipc } from "../ipc";
  import { app } from "../stores/app.svelte";
  import { models } from "../stores/models.svelte";
  import { queue, type QueueItem } from "../stores/queue.svelte";
  import { settings } from "../stores/settings.svelte";
  import type { SkipReason } from "../types";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { item, onview }: { item: QueueItem; onview: (item: QueueItem) => void } = $props();

  const stageKey: Record<string, MessageKey> = {
    preparing: "stage.preparing",
    decoding: "stage.decoding",
    upscaling: "stage.upscaling",
    encoding: "stage.encoding",
    finalizing: "stage.finalizing",
  };

  function skipText(reason: SkipReason | null, fallback: string | null): string {
    if (!reason) return fallback ?? "";
    if (reason.code === "unsupported") return t("skip.unsupported");
    if (reason.code === "output_exists") return t("skip.output_exists");
    return t("skip.video_unavailable", { detail: reason.detail });
  }

  let target = $derived.by(() => {
    if (item.outSize) return item.outSize;
    if (item.width === null || item.height === null) return null;
    return [Math.round(item.width * settings.scale), Math.round(item.height * settings.scale)] as const;
  });

  // Estimated processing time from a benchmark on this GPU, when there is one.
  let eta = $derived.by(() => {
    if (!item.pending || !item.width || !item.height) return null;
    const model = settings.currentModel;
    if (!model) return null;
    const native = model.scales.find((s) => s >= settings.scale - 1e-6) ?? model.scales.at(-1)!;
    const tp = models.throughput(model.id, native);
    if (!tp) return null;
    const mp = (item.width * item.height) / 1e6;
    const frames = item.kind === "video" ? (item.frames ?? 0) : 1;
    const batches = item.kind === "video" ? Math.max(1, Math.ceil(frames / (app.boot?.config.video.batch_frames ?? 48))) : 1;
    return { seconds: tp.value.startup * batches + tp.value.per_megapixel * mp * frames, local: tp.local };
  });

  let tone = $derived(
    item.status === "done" ? "ok" : item.status === "failed" ? "err" : item.status === "running" ? "accent" : "muted",
  ) as "ok" | "err" | "accent" | "muted";
</script>

<li class="row" class:failed={item.status === "failed"}>
  <div class="thumb">
    {#if item.thumb}
      <img src={convertFileSrc(item.thumb)} alt="" loading="lazy" decoding="async" />
    {:else}
      <Icon name={item.kind === "video" ? "film" : "image"} size={22} />
    {/if}
  </div>

  <div class="body">
    <div class="line">
      <span class="name" title={item.path}>{item.name}</span>
      <span class="meta">
        {#if item.width}{item.width}×{item.height}{/if}
        {#if target}<span class="arrow">→</span>{target[0]}×{target[1]}{/if}
        {#if item.kind === "video" && item.frames}· {t("queue.frames", { count: item.frames })}{/if}
        · {bytes(item.size)}
        {#if eta}<span class="eta" title={t(eta.local ? "queue.eta_local" : "queue.eta_reference")}>· ≈ {duration(eta.seconds * 1000)}</span>{/if}
      </span>
    </div>

    <div class="status">
      {#if item.status === "running"}
        <span class="stage">{item.stage ? t(stageKey[item.stage]) : t("queue.starting")}</span>
        {#if item.units}<span class="faint">{item.units[0]}/{item.units[1]}</span>{/if}
        <span class="pct">{Math.round(item.progress * 100)}%</span>
      {:else if item.status === "queued"}
        <span class="faint">{t("queue.queued")}</span>
      {:else if item.status === "done"}
        <span class="ok"><Icon name="check" size={13} /> {t("queue.done")}</span>
        {#if item.elapsed !== null}<span class="faint">{duration(item.elapsed)}</span>{/if}
      {:else if item.status === "failed"}
        <span class="err" title={item.message ?? ""}>{item.message ?? t("queue.failed")}</span>
      {:else if item.status === "cancelled"}
        <span class="faint">{t("queue.cancelled")}</span>
      {:else if item.status === "skipped"}
        {@const why = skipText(item.skip, item.message)}
        <span class="warn" title={why}>{t("queue.skipped", { reason: why })}</span>
      {:else}
        <span class="faint">{t("queue.ready")}</span>
      {/if}
    </div>

    {#if item.status === "running" || item.status === "queued" || item.status === "done"}
      <ProgressBar value={item.progress} {tone} />
    {/if}
  </div>

  <div class="actions">
    {#if item.status === "done" && item.output}
      <button class="ghost" title={t("queue.view")} onclick={() => onview(item)}>
        <Icon name="compare" />
      </button>
      <button class="ghost" title={t("queue.reveal")} onclick={() => ipc.reveal(item.output!)}>
        <Icon name="reveal" />
      </button>
    {/if}
    {#if item.active}
      <button class="ghost" title={t("common.cancel")} onclick={() => queue.cancel(item)}><Icon name="stop" /></button>
    {:else}
      <button class="ghost" title={t("queue.remove")} onclick={() => queue.remove(item)}><Icon name="x" /></button>
    {/if}
  </div>
</li>

<style>
  .row {
    display: grid;
    grid-template-columns: 52px 1fr auto;
    gap: 12px;
    align-items: center;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--panel);
    border: 1px solid var(--border);
    contain: layout paint;
  }
  .row.failed {
    border-color: color-mix(in srgb, var(--err) 35%, var(--border));
  }
  .thumb {
    width: 52px;
    height: 52px;
    border-radius: var(--radius-sm);
    background: var(--panel-3);
    display: grid;
    place-items: center;
    overflow: hidden;
    color: var(--faint);
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .body {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }
  .name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex-shrink: 1;
    min-width: 0;
  }
  .meta {
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .eta {
    color: var(--accent-2);
  }
  .arrow {
    margin: 0 4px;
    color: var(--faint);
  }
  .status {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
    min-width: 0;
    font-variant-numeric: tabular-nums;
  }
  .status span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .stage {
    color: var(--text);
  }
  .pct {
    margin-left: auto;
    color: var(--muted);
  }
  .ok {
    color: var(--ok);
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .err {
    color: var(--err);
  }
  .warn {
    color: var(--warn);
  }
  .actions {
    display: flex;
    gap: 2px;
  }
</style>
