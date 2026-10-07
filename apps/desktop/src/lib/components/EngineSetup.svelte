<script lang="ts">
  import { bytes } from "../format";
  import { t, tx } from "../i18n/index.svelte";
  import { errorText, ipc } from "../ipc";
  import { app } from "../stores/app.svelte";
  import type { EngineView, InstallProgress } from "../types";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { engine }: { engine: EngineView } = $props();

  let progress = $state<InstallProgress | null>(null);
  let error = $state<string | null>(null);

  const problem = $derived(engine.status.state === "missing" ? engine.status.hint : engine.status.state === "broken" ? engine.status.reason : "");

  async function install() {
    error = null;
    progress = { phase: "resolving" };
    try {
      await ipc.installEngine(engine.info.id, (p) => (progress = p));
      await app.load();
    } catch (e) {
      error = errorText(e);
    } finally {
      progress = null;
    }
  }

  const label = $derived.by(() => {
    switch (progress?.phase) {
      case "resolving":
        return t("engine.resolving");
      case "downloading":
        return progress.total
          ? t("engine.downloading_of", { received: bytes(progress.received), total: bytes(progress.total) })
          : t("engine.downloading", { received: bytes(progress.received) });
      case "extracting":
        return t("engine.installing");
      default:
        return "";
    }
  });
  const fraction = $derived(
    progress?.phase === "downloading" && progress.total ? progress.received / progress.total : progress?.phase === "extracting" ? 1 : 0,
  );
</script>

<div class="setup">
  <div class="icon"><Icon name="sparkle" size={20} /></div>
  <div class="text">
    <strong>{t("engine.not_installed", { name: engine.info.name })}</strong>
    <p class="muted">{tx(engine.info.description)}</p>
    {#if !progress}<p class="faint mono">{problem}</p>{/if}
    {#if progress}
      <div class="progress">
        <ProgressBar value={fraction} />
        <span class="muted">{label}</span>
      </div>
    {/if}
    {#if error}<p class="err">{error}</p>{/if}
  </div>
  {#if engine.installable}
    <button class="primary" disabled={progress !== null} onclick={install}>
      <Icon name="download" /> {t("engine.install")}
    </button>
  {/if}
</div>

<style>
  .setup {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 14px;
    align-items: start;
    padding: 14px 16px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--border));
    background: color-mix(in srgb, var(--accent) 8%, var(--panel));
  }
  .icon {
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: var(--accent-grad);
    color: #fff;
    display: grid;
    place-items: center;
  }
  .text p {
    margin: 4px 0 0;
  }
  .progress {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
  }
  .err {
    color: var(--err);
  }
</style>
