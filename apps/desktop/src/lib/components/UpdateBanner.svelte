<script lang="ts">
  // Announces a new version on every page. Updating goes through the dialog.
  import { t } from "../i18n/index.svelte";
  import { updates } from "../stores/updates.svelte";
  import Icon from "./Icon.svelte";

  const view = $derived(updates.available);
</script>

{#if view && !updates.dismissed}
  <div class="banner" role="status">
    <Icon name="sparkle" />
    <div class="text">
      <strong>{t("update.available", { version: view.latest })}</strong>
      <span class="muted">{t("update.current", { version: view.current })}</span>
    </div>
    <button class="primary" onclick={() => updates.ask()}><Icon name="download" /> {t("update.update")}</button>
    <button class="ghost" title={t("update.later")} aria-label={t("update.later")} onclick={() => (updates.dismissed = true)}>
      <Icon name="x" />
    </button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 10px 14px 0;
    padding: 10px 14px;
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
</style>
