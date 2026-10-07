<script lang="ts">
  import { bytes } from "../format";
  import { t } from "../i18n/index.svelte";
  import { updates } from "../stores/updates.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  const view = $derived(updates.available);
  const dl = $derived(updates.download);
</script>

{#if view && !updates.dismissed}
  <div class="banner">
    <Icon name="sparkle" />
    <div class="text">
      <strong>{t("update.available", { version: view.latest })}</strong>
      <span class="muted">
        {t("update.current", { version: view.current })}{view.asset ? " " + t("update.installer_size", { size: bytes(view.asset.size) }) : ""}
      </span>
      {#if dl}
        <ProgressBar value={dl.total ? dl.received / dl.total : 0} />
      {/if}
      {#if updates.installer && !dl}
        <span class="muted">{t("update.installer_opened")}</span>
      {/if}
      {#if updates.error}<span class="err">{updates.error}</span>{/if}
    </div>
    {#if view.asset}
      <button class="primary" disabled={dl !== null} onclick={() => updates.fetch()}>
        <Icon name="download" /> {dl ? t("update.downloading") : t("update.download_install")}
      </button>
    {/if}
    <button class="ghost" onclick={() => updates.skip()}>{t("update.skip")}</button>
    <button class="ghost" title={t("update.later")} onclick={() => (updates.dismissed = true)}><Icon name="x" /></button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    margin: 0 0 12px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent-2) 9%, var(--panel));
    border: 1px solid color-mix(in srgb, var(--accent-2) 35%, var(--border));
    color: var(--accent-2);
  }
  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--text);
    min-width: 0;
  }
  .err {
    color: var(--err);
  }
</style>
