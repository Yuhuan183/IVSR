<script lang="ts">
  // Tries filters on the picture in the viewer: post-processing after
  // super-resolution (with the picture before it as reference) or
  // pre-processing before it. Previews render in the core and come back as
  // cached files; the viewer shows the filtered picture on the right.
  import { save } from "@tauri-apps/plugin-dialog";
  import { basename } from "../format";
  import { t } from "../i18n/index.svelte";
  import { errorText, ipc } from "../ipc";
  import { FILTER_PANEL, layout } from "../stores/layout.svelte";
  import { settings } from "../stores/settings.svelte";
  import { viewer, type FilterBase, type FilterTarget, type ViewerItem } from "../stores/viewer.svelte";
  import type { FilterStep } from "../types";
  import FilterChainEditor from "./FilterChainEditor.svelte";
  import Icon from "./Icon.svelte";
  import Resizer from "./Resizer.svelte";
  import Switch from "./Switch.svelte";

  let { item, sides }: { item: ViewerItem; sides: [string, string] } = $props();

  const PREVIEW_DELAY_MS = 250;

  let notice = $state<string | null>(null);
  /** Saving is separate from the preview's busy state, which the preview owns. */
  let saving = $state(false);

  const stage = $derived(viewer.stage);
  const stageName = $derived(t(stage === "pre" ? "filters.pre" : "filters.post"));
  const steps = $derived(viewer.steps[stage] ?? structuredClone($state.snapshot(settings.steps(stage))));
  const active = $derived(steps.filter((s) => s.enabled));
  const path = $derived(viewer.target === "result" ? item.result : item.original);
  const reference = $derived(viewer.target === "result" ? item.original : null);
  const names = $derived({ result: t("viewer.result"), original: t("viewer.original") });
  const targetName = $derived(names[viewer.target]);
  const otherName = $derived(names[viewer.target === "result" ? "original" : "result"]);

  // Re-render the preview when the picture, target or steps change; only the
  // newest request may set the result. A new picture drops the old preview at
  // once; parameter edits keep it on screen until the next one is ready.
  let request = 0;
  let shown: string | null = null;
  $effect(() => {
    const id = ++request;
    const job = { path, reference, stage, steps: $state.snapshot(active) as FilterStep[] };
    viewer.activeSteps = job.steps.length;
    notice = null;
    if (job.path !== shown) {
      viewer.processed = null;
      shown = job.path;
    }
    if (!viewer.filtering || job.steps.length === 0) {
      viewer.processed = null;
      viewer.busy = false;
      viewer.error = null;
      return;
    }
    viewer.busy = true;
    const timer = setTimeout(async () => {
      try {
        const file = await ipc.filterPreview(job.path, job.reference, job.stage, job.steps);
        if (id === request) {
          viewer.processed = file;
          viewer.error = null;
        }
      } catch (e) {
        if (id === request) {
          viewer.processed = null;
          viewer.error = errorText(e);
        }
      } finally {
        if (id === request) viewer.busy = false;
      }
    }, PREVIEW_DELAY_MS);
    return () => clearTimeout(timer);
  });

  $effect(() => () => {
    viewer.processed = null;
    viewer.busy = false;
    viewer.error = null;
  });

  function setSteps(next: FilterStep[] | null) {
    viewer.steps = { ...viewer.steps, [stage]: next };
  }

  async function saveAs() {
    const name = basename(path);
    const dot = name.lastIndexOf(".");
    const [stem, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot + 1)] : [name, "png"];
    const dir = path.slice(0, path.length - name.length);
    const output = await save({ defaultPath: `${dir}${stem}_${stage}.${ext}`, title: t("viewer.filters_save") });
    if (!output) return;
    saving = true;
    try {
      const outcome = await ipc.filterSave(path, reference, stage, $state.snapshot(active) as FilterStep[], output);
      notice = t("viewer.filters_saved", { name: basename(outcome.output) });
      viewer.error = null;
    } catch (e) {
      viewer.error = errorText(e);
    } finally {
      saving = false;
    }
  }

  function useInWorkflow() {
    settings.setSteps(stage, structuredClone($state.snapshot(steps)) as FilterStep[]);
    notice = t("viewer.filters_used", { stage: stageName });
  }

  const targets: FilterTarget[] = ["original", "result"];
  const bases: FilterBase[] = ["unfiltered", "other"];
</script>

<aside class="panel" style:--w="{layout.filterPanelWidth}px" aria-label={t("viewer.filters_title")}>
  <Resizer
    width={layout.filterPanelWidth}
    min={FILTER_PANEL.min}
    max={FILTER_PANEL.max}
    initial={FILTER_PANEL.initial}
    onresize={(w) => (layout.filterPanelWidth = w)}
    oncommit={() => layout.persist()}
  />
  <header>
    <h3>{t("viewer.filters_title")}</h3>
    {#if item.kind === "image"}
      <Switch
        checked={viewer.applied}
        label={t("viewer.filters_apply")}
        title={t("viewer.filters_apply_hint")}
        onchange={(on) => (viewer.applied = on)}
      />
    {/if}
    <button class="ghost icon" title={t("common.close")} onclick={() => (viewer.panel = false)}><Icon name="x" size={14} /></button>
  </header>

  {#if item.kind === "video"}
    <p class="muted">{t("viewer.filters_video")}</p>
  {:else}
    <div class="group" class:dim={!viewer.applied}>
      <span class="label">{t("viewer.filters_target")}</span>
      <div class="pair">
        <div class="segmented" role="radiogroup" aria-label={t("viewer.filters_target")}>
          {#each targets as id (id)}
            <button role="radio" aria-checked={viewer.target === id} class:on={viewer.target === id} onclick={() => (viewer.target = id)}>
              {names[id]}
            </button>
          {/each}
        </div>
        <button
          class="ghost icon save"
          title={t("viewer.filters_save")}
          aria-label={t("viewer.filters_save")}
          disabled={saving || active.length === 0}
          onclick={saveAs}><Icon name="download" size={15} /></button
        >
      </div>

      <span class="label">{t("viewer.filters_compare")}</span>
      <div class="segmented" role="radiogroup" aria-label={t("viewer.filters_compare")}>
        {#each bases as id (id)}
          <button role="radio" aria-checked={viewer.base === id} class:on={viewer.base === id} onclick={() => (viewer.base = id)}>
            {id === "unfiltered" ? t("viewer.filters_unfiltered", { name: targetName }) : otherName}
          </button>
        {/each}
      </div>
      <p class="sides">{t("viewer.filters_sides", { left: sides[0], right: sides[1] })}</p>
      <p class="hint">{t(viewer.target === "result" ? "viewer.filters_result_hint" : "viewer.filters_source_hint")}</p>
    </div>

    <FilterChainEditor
      {stage}
      {steps}
      idPrefix="viewer-"
      onchange={setSteps}
      onreset={viewer.steps[stage] ? () => setSteps(null) : undefined}
    />

    {#if viewer.error}<p class="err">{viewer.error}</p>{/if}
    {#if notice}<p class="ok">{notice}</p>{/if}

    <footer>
      <button class="ghost" title={t("viewer.filters_use_hint", { stage: stageName })} onclick={useInWorkflow}>
        <Icon name="layers" size={14} />
        {t("viewer.filters_use", { stage: stageName })}
      </button>
    </footer>
  {/if}
</aside>

<style>
  .panel {
    position: relative;
    width: var(--w);
    flex-shrink: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px 14px;
    background: var(--panel);
    border-left: 1px solid var(--border);
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  h3 {
    flex: 1;
    margin: 0;
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  .icon {
    padding: 4px;
  }
  .group {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 8px 10px;
    transition: opacity 120ms;
  }
  .group.dim {
    opacity: 0.55;
  }
  .label {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }
  .pair {
    display: flex;
    gap: 4px;
    min-width: 0;
  }
  .pair .segmented {
    flex: 1;
  }
  .save {
    padding: 4px 6px;
    border: 1px solid var(--border);
  }
  .sides,
  .hint {
    grid-column: 1 / -1;
    margin: 0;
    font-size: 11.5px;
  }
  .sides {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .hint {
    color: var(--faint);
  }
  .segmented {
    display: grid;
    grid-template-columns: 1fr 1fr;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .segmented button {
    min-width: 0;
    border: none;
    border-radius: 0;
    background: var(--panel-2);
    justify-content: center;
    padding: 5px 6px;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .segmented button + button {
    border-left: 1px solid var(--border);
  }
  .segmented button.on {
    background: var(--accent-grad);
    color: #fff;
  }
  footer {
    display: flex;
    gap: 6px;
    margin-top: auto;
  }
  .err {
    margin: 0;
    color: var(--err);
    font-size: 12px;
    user-select: text;
  }
  .ok {
    margin: 0;
    color: var(--ok);
    font-size: 12px;
  }
  /* Narrow windows (or a large content scale): float over the picture. */
  @media (max-width: 760px) {
    .panel {
      position: absolute;
      top: 0;
      right: 0;
      bottom: 0;
      width: min(var(--w), 88vw);
      z-index: 6;
      box-shadow: var(--shadow);
    }
  }
</style>
