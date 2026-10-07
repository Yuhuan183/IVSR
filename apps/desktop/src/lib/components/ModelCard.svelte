<script lang="ts">
  import { bytes, duration } from "../format";
  import { i18n, t, tx } from "../i18n/index.svelte";
  import { app } from "../stores/app.svelte";
  import { models } from "../stores/models.svelte";
  import { settings } from "../stores/settings.svelte";
  import type { HardwareProfile, ModelEntry } from "../types";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { entry, disabled }: { entry: ModelEntry; disabled: boolean } = $props();

  let expanded = $state(false);

  const FRAME_MP = (1920 * 1080) / 1e6;

  const info = $derived(entry.installed);
  const manifest = $derived(entry.manifest);
  const name = $derived(info?.name ?? manifest?.name ?? entry.id);
  const description = $derived(tx(info?.description ?? manifest?.description));
  const scales = $derived(info?.scales ?? manifest?.scales ?? []);
  const tags = $derived(info?.tags ?? manifest?.tags ?? []);
  const license = $derived(info?.license ?? manifest?.license);
  const author = $derived(info?.author ?? manifest?.author);
  const homepage = $derived(info?.homepage ?? manifest?.homepage);
  const size = $derived(info?.size ?? manifest?.files.reduce((a, f) => a + f.size, 0) ?? 0);
  const busy = $derived(models.busy[entry.id]);
  const isDefault = $derived(settings.currentModel?.id === entry.id && settings.engine === models.engine);

  const profile = $derived.by((): HardwareProfile | null => {
    if (info?.hardware) return info.hardware;
    const arch = manifest?.architecture;
    return (arch && models.overview?.architectures[arch]) || null;
  });

  const reference = $derived(entry.reference_throughput);
  const local = $derived(
    app.boot?.benchmarks.find((b) => b.model === entry.id && b.engine === models.engine && (!models.activeGpu || b.device === models.activeGpu.name)),
  );

  // Computed by the core against the GPU the configured parameters select.
  const advice = $derived.by(() => {
    const a = models.overview?.advice[entry.id];
    if (!a || a.fit === "unknown" || a.needed_mb === null || a.available_mb === null) return null;
    return { fit: a.fit, needed: a.needed_mb, available: a.available_mb, tile: a.suggested_tile ?? 0 };
  });

  const statusKey = {
    bundled: "models.status_bundled",
    installed: "models.status_installed",
    update_available: "models.status_update",
    imported: "models.status_imported",
    available: "models.status_available",
  } as const;
  const classKey = { light: "class.light", medium: "class.medium", heavy: "class.heavy" } as const;
  const cls = $derived(info?.class ?? profile?.class ?? null);

  function useModel() {
    settings.engine = models.engine;
    settings.model = entry.id;
    if (!scales.includes(settings.scale) && scales.length) settings.scale = scales[0];
  }

  function mb(v: number) {
    return bytes(v * 1024 * 1024);
  }
</script>

<li class="card" class:default={isDefault}>
  <div class="main">
    <div class="title">
      <strong>{name}</strong>
      <span class="id mono">{entry.id}</span>
      <span class="status {entry.status}">{t(statusKey[entry.status])}</span>
      {#if isDefault}<span class="status default">{t("models.default")}</span>{/if}
    </div>
    <p class="desc">{description}</p>
    <div class="chips">
      {#each scales as s (s)}<span class="chip scale">×{s}</span>{/each}
      {#if cls}<span class="chip class {cls}">{t(classKey[cls])}</span>{/if}
      {#each tags as tag (tag)}<span class="chip">{tag}</span>{/each}
      <span class="chip">{bytes(size)}</span>
      {#if license}<span class="chip">{license}</span>{/if}
    </div>
    {#if busy}
      <div class="busy">
        <ProgressBar value={busy.progress} />
        <span class="faint">{t(busy.kind === "bench" ? "models.benchmarking" : busy.kind === "install" ? "models.installing" : "models.removing")}</span>
      </div>
    {/if}
  </div>

  <div class="actions">
    {#if entry.status === "available"}
      <button class="primary" disabled={disabled || !!busy} onclick={() => models.install(entry.id)}>
        <Icon name="download" /> {t("models.install")}
      </button>
    {:else}
      {#if entry.status === "update_available"}
        <button class="primary" disabled={disabled || !!busy} onclick={() => models.install(entry.id)}>
          <Icon name="download" /> {t("models.update")}
        </button>
      {/if}
      <button disabled={isDefault} onclick={useModel}><Icon name="check" /> {t("models.use")}</button>
      <button disabled={disabled || !!busy} onclick={() => models.bench(entry.id)} title={t("models.bench_hint")}>
        <Icon name="gauge" /> {t("models.bench")}
      </button>
      {#if info?.removable}
        <button class="danger" disabled={disabled || !!busy} onclick={() => models.remove(entry.id)} title={t("models.remove")}>
          <Icon name="trash" />
        </button>
      {/if}
    {/if}
    <button class="ghost" aria-expanded={expanded} onclick={() => (expanded = !expanded)}>
      <span class="chev" class:open={expanded}><Icon name="chevron" size={12} /></span> {t("models.details")}
    </button>
  </div>

  <div class="perf">
    {#if local}
      <span title={t("models.local_hint")}>
        <Icon name="gauge" size={12} /> {t("models.per_frame_local", { time: duration(1000 * (local.throughput.startup + local.throughput.per_megapixel * FRAME_MP)) })}
      </span>
    {:else if reference}
      <span class="faint" title={t("models.reference_hint")}>
        {t("models.per_frame_reference", { time: duration(1000 * (reference.startup + reference.per_megapixel * FRAME_MP)) })}
      </span>
    {/if}
    {#if advice}
      <span class="advice {advice.fit}">
        {advice.fit === "comfortable"
          ? t("advice.comfortable_short")
          : advice.fit === "constrained"
            ? t("advice.constrained_short", { tile: advice.tile })
            : t("advice.insufficient_short")}
      </span>
    {/if}
  </div>

  {#if expanded}
    <div class="details">
      <div>
        <h4>{t("models.hardware")}</h4>
        {#if profile}
          <p class="muted">{tx(profile.summary)}</p>
          <table>
            <thead><tr><th>{t("models.tile")}</th><th>{t("models.peak_memory")}</th></tr></thead>
            <tbody>
              {#each profile.memory_by_tile as row (row.tile)}
                <tr class:suggested={advice?.fit === "constrained" && advice.tile === row.tile}>
                  <td>{row.tile === 0 ? t("models.tile_auto") : row.tile}</td>
                  <td>{mb(row.mb)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if advice}
            <p class="advice-text {advice.fit}">
              {advice.fit === "comfortable"
                ? t("advice.comfortable", { gpu: models.activeGpu?.name ?? "", available: mb(advice.available), needed: mb(advice.needed) })
                : advice.fit === "constrained"
                  ? t("advice.constrained", { gpu: models.activeGpu?.name ?? "", available: mb(advice.available), needed: mb(advice.needed), tile: advice.tile })
                  : t("advice.insufficient", { gpu: models.activeGpu?.name ?? "", available: mb(advice.available), tile: advice.tile })}
            </p>
          {:else if profile.memory_by_tile.length}
            <p class="faint">{t("advice.unknown", { needed: mb(profile.memory_by_tile[0].mb) })}</p>
          {/if}
        {:else}
          <p class="faint">{t("models.no_profile")}</p>
        {/if}
      </div>
      <div>
        <h4>{t("models.baseline")}</h4>
        {#if reference}
          <p class="muted">{t("models.reference_device", { device: models.overview?.reference.device ?? "" })}</p>
          <p>{t("models.throughput", { startup: i18n.number(reference.startup, 2), rate: i18n.number(reference.per_megapixel, 2) })}</p>
        {/if}
        {#if local}
          <p class="muted">{t("models.local_device", { device: local.device, date: new Date(local.measured_at * 1000).toLocaleDateString() })}</p>
          <p>{t("models.throughput", { startup: i18n.number(local.throughput.startup, 2), rate: i18n.number(local.throughput.per_megapixel, 2) })}</p>
        {:else if info}
          <p class="faint">{t("models.no_local")}</p>
        {/if}
        <h4>{t("models.about")}</h4>
        {#if author}<p>{t("models.author", { author })}</p>{/if}
        {#if license?.startsWith("CC-BY") && author}<p class="warn">{t("models.attribution", { author, homepage: homepage ?? "" })}</p>{/if}
        {#if homepage}<p class="mono faint link">{homepage}</p>{/if}
        {#if info?.parameters}<p class="faint">{t("models.weights", { count: i18n.number(info.parameters / 1e6, 1) })}</p>{/if}
        {#if entry.source}<p class="faint">{t("models.source", { source: entry.source === "built-in" ? t("models.builtin") : entry.source })}</p>{/if}
      </div>
    </div>
  {/if}
</li>

<style>
  .card {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px 16px;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--panel);
    border: 1px solid var(--border);
    contain: layout paint;
  }
  .card.default {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  }
  .main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
  }
  .id {
    color: var(--faint);
  }
  .status {
    font-size: 11px;
    padding: 1px 8px;
    border-radius: 99px;
    background: var(--panel-3);
    color: var(--muted);
  }
  .status.installed {
    color: var(--ok);
  }
  .status.update_available {
    color: var(--accent-2);
    font-weight: 600;
  }
  .status.imported {
    color: #c084fc;
  }
  .status.available {
    color: var(--warn);
  }
  .status.default {
    color: #fff;
    background: var(--accent-grad);
  }
  .desc {
    margin: 0;
    color: var(--muted);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    font-size: 11px;
    padding: 1px 7px;
    border-radius: 99px;
    background: var(--panel-3);
    color: var(--muted);
  }
  .chip.scale {
    color: var(--accent-2);
  }
  .chip.class.light {
    color: var(--ok);
  }
  .chip.class.medium {
    color: var(--warn);
  }
  .chip.class.heavy {
    color: var(--err);
  }
  .busy {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    max-width: 360px;
  }
  .actions {
    display: flex;
    align-items: flex-start;
    gap: 6px;
  }
  .perf {
    grid-column: 1 / -1;
    display: flex;
    gap: 14px;
    font-size: 12px;
    align-items: center;
  }
  .perf span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .advice.comfortable,
  .advice-text.comfortable {
    color: var(--ok);
  }
  .advice.constrained,
  .advice-text.constrained {
    color: var(--warn);
  }
  .advice.insufficient,
  .advice-text.insufficient {
    color: var(--err);
  }
  .details {
    grid-column: 1 / -1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    font-size: 12.5px;
  }
  .details p {
    margin: 0 0 4px;
  }
  h4 {
    margin: 4px 0 6px;
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }
  table {
    border-collapse: collapse;
    margin: 6px 0;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    text-align: left;
    padding: 2px 14px 2px 0;
  }
  th {
    color: var(--faint);
    font-weight: 500;
  }
  tr.suggested td {
    color: var(--warn);
    font-weight: 600;
  }
  .warn {
    color: var(--warn);
  }
  .link {
    user-select: text;
    word-break: break-all;
  }
  .chev {
    display: inline-flex;
    transition: transform 120ms;
  }
  .chev.open {
    transform: rotate(90deg);
  }
</style>
