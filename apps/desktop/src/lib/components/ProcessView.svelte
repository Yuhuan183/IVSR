<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { basename } from "../format";
  import { t } from "../i18n/index.svelte";
  import { app } from "../stores/app.svelte";
  import { queue, type QueueItem } from "../stores/queue.svelte";
  import { settings } from "../stores/settings.svelte";
  import { viewer, type ViewerItem } from "../stores/viewer.svelte";
  import EmptyState from "./EmptyState.svelte";
  import EngineSetup from "./EngineSetup.svelte";
  import Icon from "./Icon.svelte";
  import QueueList from "./QueueList.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";

  let drawer = $state(false);

  const engine = $derived(settings.engineView);
  const engineReady = $derived(engine?.status.state === "ready");
  const counts = $derived(queue.counts);

  async function addFiles() {
    const picked = await open({
      multiple: true,
      title: t("process.add_files_title"),
      filters: [{ name: t("process.filter_media"), extensions: app.inputExtensions }],
    });
    if (picked) await queue.add(Array.isArray(picked) ? picked : [picked]);
  }

  async function addFolder() {
    const picked = await open({ directory: true, multiple: true, title: t("process.add_folder_title") });
    if (picked) await queue.add(Array.isArray(picked) ? picked : [picked]);
  }

  function toViewer(i: QueueItem): ViewerItem {
    return {
      kind: i.kind,
      name: basename(i.output ?? i.path),
      original: i.path,
      result: i.output!,
      sourceSize: i.width && i.height ? [i.width, i.height] : null,
      resultSize: i.outSize,
      model: settings.currentModel?.id,
      elapsedMs: i.elapsed,
    };
  }

  function view(item: QueueItem) {
    const done = queue.items.filter((i) => i.status === "done" && i.output);
    const list = done.map(toViewer);
    viewer.open(list[done.indexOf(item)], list);
  }
</script>

<div class="process">
  <section class="workspace">
    {#if engine && !engineReady}
      <EngineSetup {engine} />
    {/if}
    <div class="queue" class:empty={queue.items.length === 0}>
      {#if queue.items.length === 0}
        <EmptyState onfiles={addFiles} onfolder={addFolder} />
      {:else}
        <QueueList onview={view} />
      {/if}
    </div>
    {#if queue.error}<p class="error">{queue.error}</p>{/if}
    <footer class="actions">
      <button onclick={addFiles} disabled={queue.adding}><Icon name="plus" /> {t("process.files")}</button>
      <button onclick={addFolder} disabled={queue.adding}><Icon name="folder" /> {t("process.folder")}</button>
      {#if counts.total > 0}
        <button class="ghost" onclick={() => queue.clearFinished()} disabled={counts.total === counts.pending + counts.active}>
          {t("process.clear_finished")}
        </button>
        <button class="ghost" onclick={() => queue.clearAll()} title={t("process.clear_all_hint")}>
          <Icon name="trash" />
        </button>
      {/if}
      <span class="summary">
        {#if counts.total > 0}
          {t("process.progress", { done: counts.done, total: counts.total })}{#if counts.failed > 0}<span class="err">
              · {t("process.failed", { count: counts.failed })}</span
            >{/if}
        {/if}
      </span>
      {#if counts.active > 0}
        <button class="danger" onclick={() => queue.cancelAll()}><Icon name="stop" /> {t("process.stop")}</button>
      {/if}
      <button class="settings-toggle" aria-expanded={drawer} onclick={() => (drawer = !drawer)}>
        <Icon name="sliders" /> {t("process.settings")}
      </button>
      <button class="primary" disabled={!engineReady || counts.pending === 0} onclick={() => queue.start(settings.request())}>
        <Icon name="play" />
        {counts.pending > 0 ? t("process.upscale_count", { count: counts.pending }) : t("process.upscale")}
      </button>
    </footer>
  </section>
  <div class="settings" class:open={drawer}>
    <SettingsPanel onclose={() => (drawer = false)} />
  </div>
  {#if drawer}<button class="scrim" aria-label={t("common.close")} onclick={() => (drawer = false)}></button>{/if}
</div>

<style>
  .process {
    position: relative;
    height: 100%;
    display: grid;
    grid-template-columns: 1fr auto;
    min-height: 0;
  }
  .settings {
    display: flex;
    min-height: 0;
  }
  .settings-toggle,
  .scrim {
    display: none;
  }
  /* Narrow windows (or a large content scale): settings slide over the queue. */
  @media (max-width: 760px) {
    .process {
      grid-template-columns: 1fr;
    }
    .settings {
      position: absolute;
      top: 0;
      right: 0;
      bottom: 0;
      z-index: 8;
      max-width: 92vw;
      box-shadow: var(--shadow);
      transform: translateX(105%);
      transition: transform 160ms ease-out;
    }
    .settings.open {
      transform: none;
    }
    .settings-toggle {
      display: inline-flex;
    }
    .scrim {
      display: block;
      position: absolute;
      inset: 0;
      z-index: 7;
      border: none;
      border-radius: 0;
      background: rgb(0 0 0 / 0.35);
    }
  }
  .workspace {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 14px;
    gap: 12px;
  }
  .queue {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-right: 2px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .summary {
    flex: 1;
    text-align: right;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .error {
    margin: 0;
    color: var(--err);
  }
  .err {
    color: var(--err);
  }
</style>
