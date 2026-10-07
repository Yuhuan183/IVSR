<script lang="ts">
  import { onMount } from "svelte";
  import { bytes } from "../format";
  import { t } from "../i18n/index.svelte";
  import { models } from "../stores/models.svelte";
  import { queue } from "../stores/queue.svelte";
  import Icon from "./Icon.svelte";
  import ImportDialog from "./ImportDialog.svelte";
  import ModelCard from "./ModelCard.svelte";

  let importing = $state(false);
  let filter = $state<"all" | "installed" | "available">("all");

  onMount(() => {
    void models.load();
    if (!models.system) void models.loadSystem();
  });

  const entries = $derived(
    (models.overview?.entries ?? []).filter((e) =>
      filter === "all" ? true : filter === "installed" ? e.status !== "available" : e.status === "available",
    ),
  );
  const sys = $derived(models.system?.system);
  const busyQueue = $derived(queue.counts.active > 0);
  const updates = $derived((models.overview?.entries ?? []).filter((e) => e.status === "update_available").length);
</script>

<section class="models">
  <div class="top">
    <div class="system card">
      <div class="head"><Icon name="cpu" /> <strong>{t("models.this_machine")}</strong></div>
      {#if sys}
        <dl>
          <dt>{t("models.cpu")}</dt>
          <dd>{sys.cpu ?? t("common.unknown")} · {t("models.threads", { count: sys.threads })}</dd>
          <dt>{t("models.memory")}</dt>
          <dd>{sys.memory_mb ? bytes(sys.memory_mb * 1024 * 1024) : t("common.unknown")}</dd>
          <dt>{t("models.gpu")}</dt>
          <dd>
            {#each sys.gpus as g (g.index)}
              <div>
                {g.name}
                <span class="faint">
                  {g.unified ? t("models.unified") : g.memory_mb ? bytes(g.memory_mb * 1024 * 1024) : t("models.memory_unknown")}
                </span>
              </div>
            {:else}
              <span class="faint">{t("models.no_gpu")}</span>
            {/each}
          </dd>
        </dl>
      {:else}
        <p class="faint">{t("models.detecting")}</p>
      {/if}
    </div>
    <div class="intro card">
      <p>{t("models.intro")}</p>
      {#if models.overview}
        <p class="faint">{t("models.reference_note", { device: models.overview.reference.device, date: models.overview.reference.measured_on })}</p>
      {/if}
      {#if busyQueue}<p class="warn">{t("models.queue_busy")}</p>{/if}
    </div>
  </div>

  <div class="toolbar">
    <div class="segmented" role="radiogroup">
      {#each [["all", "models.filter_all"], ["installed", "models.filter_installed"], ["available", "models.filter_available"]] as const as [id, key] (id)}
        <button role="radio" aria-checked={filter === id} class:on={filter === id} onclick={() => (filter = id)}>{t(key)}</button>
      {/each}
    </div>
    {#if updates > 0}<span class="update">{t("models.updates_available", { count: updates })}</span>{/if}
    <span class="spacer"></span>
    <button class="ghost" disabled={models.loading} onclick={() => models.load(true)} title={t("models.refresh_hint")}>
      <Icon name="refresh" /> {t("models.refresh")}
    </button>
    <button disabled={busyQueue} onclick={() => (importing = true)}><Icon name="upload" /> {t("models.import")}</button>
  </div>

  {#if models.error}<p class="err">{models.error === "busy" ? t("models.queue_busy") : models.error}</p>{/if}
  {#each models.overview?.catalog_errors ?? [] as [url, reason] (url)}
    <p class="warn">{t("models.catalog_error", { url, reason })}</p>
  {/each}

  <ul class="list">
    {#each entries as entry (entry.id)}
      <ModelCard {entry} disabled={busyQueue} />
    {/each}
  </ul>
</section>

{#if importing}
  <ImportDialog onclose={() => (importing = false)} />
{/if}

<style>
  .models {
    height: 100%;
    overflow-y: auto;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .top {
    display: grid;
    grid-template-columns: minmax(280px, 1fr) 2fr;
    gap: 12px;
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 14px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 12px;
    margin: 0;
    font-size: 12.5px;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
  }
  .intro p {
    margin: 0 0 6px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .spacer {
    flex: 1;
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
  .update {
    color: var(--accent-2);
    font-weight: 600;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .warn {
    color: var(--warn);
    margin: 0;
  }
  .err {
    color: var(--err);
    margin: 0;
  }
</style>
