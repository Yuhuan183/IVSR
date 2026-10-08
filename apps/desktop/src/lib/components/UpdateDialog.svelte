<script lang="ts">
  // Asks before updating, then shows the download and what happens next.
  import { bytes } from "../format";
  import { t } from "../i18n/index.svelte";
  import { app } from "../stores/app.svelte";
  import { queue } from "../stores/queue.svelte";
  import { updates } from "../stores/updates.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  const view = $derived(updates.available);
  const phase = $derived(updates.phase);
  const dl = $derived(updates.download);
  const busy = $derived(phase === "downloading" || phase === "restarting");
  const appimage = $derived(updates.mode === "appimage");
  /** Installing replaces or restarts the app, so it waits for running jobs. */
  const jobsRunning = $derived(queue.counts.active > 0);
  const os = $derived(app.boot?.platform);
  /** What will happen, shown before the user agrees. */
  const next = $derived.by(() => {
    if (appimage) return t("update.next_appimage");
    return t(os === "macos" ? "update.next_macos" : os === "windows" ? "update.next_windows" : "update.next_linux");
  });
  /** What the user does now that the installer is open. */
  const then = $derived(t(os === "macos" ? "update.then_macos" : os === "windows" ? "update.then_windows" : "update.then_linux"));

  // Escape closes only an open dialog (and not mid-download); otherwise it is
  // left to the viewer and others.
  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape" && view && phase && !busy) {
      e.stopPropagation();
      updates.close();
    }
  }
</script>

<svelte:window onkeydowncapture={keydown} />

{#if view && phase}
  <div class="backdrop" role="presentation" onclick={() => updates.close()}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="update-title" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
      <header>
        <h2 id="update-title">{t("update.confirm_title", { version: view.latest })}</h2>
        {#if !busy}<button class="ghost" title={t("common.close")} onclick={() => updates.close()}><Icon name="x" /></button>{/if}
      </header>

      <p class="muted">
        {t("update.current", { version: view.current })}
        {#if view.asset}{t("update.download_size", { size: bytes(view.asset.size) })}{/if}
      </p>
      {#if view.notes.trim()}
        <pre class="notes">{view.notes.trim()}</pre>
      {/if}
      {#if view.page_url}<p class="faint small">{view.page_url}</p>{/if}

      {#if phase === "confirm"}
        <p>{next}</p>
        {#if !view.asset}<p class="err">{t("update.no_asset")}</p>{/if}
        {#if jobsRunning}<p class="warn">{t("update.busy")}</p>{/if}
        {#if updates.installError}<p class="err">{updates.installError}</p>{/if}
      {:else if phase === "downloading"}
        <p>{t("update.downloading")}{#if dl?.total} {bytes(dl.received)} / {bytes(dl.total)}{/if}</p>
        <ProgressBar value={dl?.total ? dl.received / dl.total : 0} />
      {:else if phase === "restarting"}
        <p>{t("update.restarting")}</p>
      {:else if phase === "opened"}
        <p class="ok">{t("update.installer_opened")}</p>
        <p>{then}</p>
      {:else if phase === "failed"}
        <p class="err">{t("update.failed", { error: updates.installError ?? "" })}</p>
      {/if}

      <footer>
        {#if phase === "confirm" || phase === "failed"}
          <button class="ghost" onclick={() => updates.skip()}>{t("update.skip")}</button>
          <span class="spacer"></span>
          <button onclick={() => updates.close()}>{t("update.later")}</button>
          <button class="primary" disabled={!view.asset || jobsRunning} onclick={() => updates.install()}>
            <Icon name="download" /> {phase === "failed" ? t("common.retry") : t("update.confirm_install")}
          </button>
        {:else if phase === "opened"}
          <span class="spacer"></span>
          <button onclick={() => updates.close()}>{t("common.close")}</button>
        {/if}
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 35;
    background: rgb(0 0 0 / 0.55);
    display: grid;
    place-items: center;
  }
  .dialog {
    width: min(520px, 92vw);
    max-height: 86vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
    padding: 16px 18px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .notes {
    margin: 0;
    max-height: 180px;
    overflow: auto;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--panel-2);
    border: 1px solid var(--border);
    font-family: inherit;
    font-size: 12px;
    white-space: pre-wrap;
    user-select: text;
  }
  .small {
    font-size: 11.5px;
    user-select: text;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
  }
  .spacer {
    flex: 1;
  }
  .err {
    color: var(--err);
    user-select: text;
  }
  .ok {
    color: var(--ok);
  }
  .warn {
    color: var(--warn);
  }
</style>
