<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import BrowseView from "./lib/components/BrowseView.svelte";
  import Icon from "./lib/components/Icon.svelte";
  import ModelsView from "./lib/components/ModelsView.svelte";
  import ProcessView from "./lib/components/ProcessView.svelte";
  import TitleBar from "./lib/components/TitleBar.svelte";
  import Viewer from "./lib/components/Viewer.svelte";
  import { i18n, t } from "./lib/i18n/index.svelte";
  import { ipc } from "./lib/ipc";
  import { app } from "./lib/stores/app.svelte";
  import { models } from "./lib/stores/models.svelte";
  import { nav } from "./lib/stores/nav.svelte";
  import { queue } from "./lib/stores/queue.svelte";
  import { settings } from "./lib/stores/settings.svelte";
  import { updates } from "./lib/stores/updates.svelte";
  import { viewer } from "./lib/stores/viewer.svelte";

  let dragging = $state(false);

  $effect(() => {
    document.documentElement.lang = i18n.locale;
    document.title = "IVSR";
  });

  onMount(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      await app.load();
      if (app.boot) settings.load(app.boot.config);
      await queue.init();
      void queue.add(await ipc.takeLaunchInputs());
      if (app.boot?.update_configured) void updates.check(false);
      // Hardware and catalogue data feed estimates; fetch them off the critical path.
      if (settings.engineView?.status.state === "ready") {
        void models.loadSystem();
        void models.load();
      }
      unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") dragging = true;
        else if (p.type === "leave") dragging = false;
        else if (p.type === "drop") {
          dragging = false;
          nav.view = "process";
          void queue.add(p.paths);
        }
      });
    })();
    return () => unlisten?.();
  });
</script>

{#if app.error && !app.boot}
  <div class="fatal">
    <h2>{t("app.start_failed")}</h2>
    <p class="mono">{app.error}</p>
    <button onclick={() => app.load()}>{t("common.retry")}</button>
  </div>
{:else}
  <div class="shell">
    <TitleBar />
    <main class="main">
      {#if nav.view === "process"}
        <ProcessView />
      {:else if nav.view === "browse"}
        <BrowseView />
      {:else}
        <ModelsView />
      {/if}
    </main>
  </div>

  {#if dragging}
    <div class="drop-overlay"><div><Icon name="upload" size={32} /> {t("app.drop")}</div></div>
  {/if}

  {#if viewer.current}
    <Viewer />
  {/if}
{/if}

<style>
  .shell {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .main {
    flex: 1;
    min-height: 0;
  }
  .drop-overlay {
    position: fixed;
    inset: 8px;
    border: 2px dashed var(--accent);
    border-radius: 16px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    display: grid;
    place-items: center;
    pointer-events: none;
    z-index: 10;
    font-size: 16px;
    font-weight: 600;
  }
  .drop-overlay div {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .fatal {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 40px;
  }
</style>
