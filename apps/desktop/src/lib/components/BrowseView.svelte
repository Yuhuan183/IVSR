<script lang="ts">
  import { onMount } from "svelte";
  import { basename, duration, scaleText } from "../format";
  import { t } from "../i18n/index.svelte";
  import { ipc } from "../ipc";
  import { history } from "../stores/history.svelte";
  import { viewer, type ViewerItem } from "../stores/viewer.svelte";
  import type { HistoryItem } from "../types";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  onMount(() => {
    if (history.stale) void history.load();
  });

  $effect(() => {
    if (history.stale && !history.loading) void history.load();
  });

  function toViewer(i: HistoryItem): ViewerItem {
    return {
      kind: i.kind,
      name: basename(i.output),
      original: i.input,
      result: i.output,
      sourceSize: [i.source_width, i.source_height],
      resultSize: [i.width, i.height],
      model: i.model,
      scale: i.scale,
      elapsedMs: i.elapsed_ms,
    };
  }

  function open(item: HistoryItem) {
    const viewable = history.visible.filter((i) => i.input_exists && i.output_exists);
    const list = viewable.map(toViewer);
    const index = viewable.indexOf(item);
    viewer.open(list[index] ?? toViewer(item), list.length ? list : [toViewer(item)]);
  }

  function when(id: number): string {
    return new Date(id).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  }

  const filters = [
    { id: "all", key: "browse.filter_all" },
    { id: "image", key: "browse.filter_images" },
    { id: "video", key: "browse.filter_videos" },
  ] as const;
</script>

<section class="browse">
  <div class="toolbar">
    <div class="segmented" role="radiogroup">
      {#each filters as f (f.id)}
        <button role="radio" aria-checked={history.filter === f.id} class:on={history.filter === f.id} onclick={() => (history.filter = f.id)}>
          {t(f.key)}
        </button>
      {/each}
    </div>
    <label class="search">
      <Icon name="search" size={14} />
      <input type="text" placeholder={t("browse.search")} bind:value={history.query} spellcheck="false" />
    </label>
    <span class="faint count">{t("browse.count", { count: history.visible.length })}</span>
    <button class="ghost" onclick={() => history.load()} title={t("common.refresh")}><Icon name="refresh" /></button>
    {#if history.items.length > 0}
      <button class="ghost" onclick={() => history.clear()} title={t("browse.clear_hint")}><Icon name="trash" /> {t("browse.clear")}</button>
    {/if}
  </div>

  {#if history.error}<p class="err">{history.error}</p>{/if}

  {#if history.items.length === 0 && !history.loading}
    <div class="empty">
      <Icon name="image" size={28} />
      <h2>{t("browse.empty_title")}</h2>
      <p class="muted">{t("browse.empty_body")}</p>
    </div>
  {:else}
    <ul class="grid">
      {#each history.visible as item (item.id)}
        {@const missing = !item.output_exists}
        <li class="card" class:missing>
          <button
            class="preview"
            disabled={missing || !item.input_exists}
            onclick={() => open(item)}
            title={item.output}
            aria-label={t("browse.open", { name: basename(item.output) })}
          >
            <Thumb path={missing ? null : item.output} kind={item.kind} />
            <span class="badge">×{scaleText(item.scale)}</span>
            {#if item.kind === "video"}<span class="badge video"><Icon name="film" size={12} /> {item.frames}</span>{/if}
          </button>
          <div class="meta">
            <strong title={item.output}>{basename(item.output)}</strong>
            <span class="muted">{item.source_width}×{item.source_height} → {item.width}×{item.height}</span>
            <span class="faint">{item.model} · {duration(item.elapsed_ms)}</span>
            <span class="faint">{when(item.id)}</span>
            {#if missing}<span class="warn">{t("browse.missing")}</span>{:else if !item.input_exists}<span class="warn">{t("browse.original_missing")}</span>{/if}
          </div>
          <div class="actions">
            {#if !missing}
              <button class="ghost" title={t("queue.reveal")} onclick={() => ipc.reveal(item.output)}><Icon name="reveal" /></button>
            {/if}
            <button class="ghost" title={t("browse.forget")} onclick={() => history.remove([item.id])}><Icon name="x" /></button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .browse {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 14px;
    gap: 12px;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
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
  .search {
    min-width: 160px;
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    max-width: 320px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--panel-2);
    color: var(--faint);
  }
  .search input {
    border: none;
    background: transparent;
    padding: 6px 0;
  }
  .count {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .grid {
    list-style: none;
    margin: 0;
    padding: 0 2px 0 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
    gap: 12px;
    align-content: start;
  }
  .card {
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    position: relative;
    contain: layout paint;
  }
  .card.missing {
    opacity: 0.6;
  }
  .preview {
    padding: 0;
    border: none;
    border-radius: 0;
    aspect-ratio: 4 / 3;
    background: var(--panel-3);
    position: relative;
    display: block;
  }
  .preview:hover:not(:disabled) {
    filter: brightness(1.08);
  }
  .badge {
    position: absolute;
    top: 8px;
    left: 8px;
    font-size: 11px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 99px;
    background: rgb(0 0 0 / 0.6);
    color: #fff;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .badge.video {
    left: auto;
    right: 8px;
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 8px 10px 10px;
    font-size: 12px;
    min-width: 0;
  }
  .meta strong,
  .meta span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta strong {
    font-size: 13px;
  }
  .actions {
    position: absolute;
    right: 4px;
    bottom: 6px;
    display: flex;
  }
  .warn {
    color: var(--warn);
  }
  .err {
    color: var(--err);
    margin: 0;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: var(--faint);
    text-align: center;
  }
  .empty h2 {
    margin: 0;
    font-size: 16px;
    color: var(--text);
  }
  .empty p {
    margin: 0;
    max-width: 380px;
  }
</style>
