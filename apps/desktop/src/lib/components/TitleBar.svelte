<script lang="ts">
  import { LOCALES, t } from "../i18n/index.svelte";
  import { app } from "../stores/app.svelte";
  import { nav, type View } from "../stores/nav.svelte";
  import { settings } from "../stores/settings.svelte";
  import { updates } from "../stores/updates.svelte";
  import Icon from "./Icon.svelte";
  import UiScaleControl from "./UiScaleControl.svelte";

  const engine = $derived(settings.engineView);
  const ready = $derived(engine?.status.state === "ready");
  const language = $derived(app.boot?.config.ui.language ?? "auto");

  const tabs: { id: View; key: "nav.process" | "nav.browse" | "nav.models"; icon: string }[] = [
    { id: "process", key: "nav.process", icon: "sparkle" },
    { id: "browse", key: "nav.browse", icon: "image" },
    { id: "models", key: "nav.models", icon: "layers" },
  ];
</script>

<header class="bar">
  <div class="brand">
    <span class="logo"><Icon name="sparkle" size={14} /></span>
    <strong>IVSR</strong>
    <span class="faint">{app.boot?.version}</span>
  </div>

  <nav class="tabs" aria-label={t("nav.label")}>
    {#each tabs as tab (tab.id)}
      <button class="tab" class:active={nav.view === tab.id} aria-current={nav.view === tab.id ? "page" : undefined} onclick={() => (nav.view = tab.id)}>
        <Icon name={tab.icon} size={14} />
        <span class="label">{t(tab.key)}</span>
      </button>
    {/each}
  </nav>

  <div class="right">
    {#if engine}
      <span class="pill" class:ready title={engine.status.state === "ready" ? engine.status.location : engine.info.name}>
        <span class="dot"></span><span class="label">{engine.info.name}</span>
        {#if engine.installed}<span class="faint">{engine.installed.release}</span>{/if}
      </span>
    {/if}
    <UiScaleControl />
    <select
      class="lang"
      value={language}
      aria-label={t("settings.language")}
      onchange={(e) => app.setLanguage((e.currentTarget as HTMLSelectElement).value)}
    >
      <option value="auto">{t("settings.language_auto")}</option>
      {#each LOCALES as l (l.id)}<option value={l.id}>{l.label}</option>{/each}
    </select>
    {#if app.boot?.update_configured}
      <button class="ghost" disabled={updates.checking} onclick={() => updates.check(true)} title={t("update.check")}>
        <span class:spin={updates.checking}><Icon name="refresh" size={14} /></span>
        {#if updates.available}<span class="update">{t("update.badge", { version: updates.available.latest })}</span>{/if}
      </button>
    {/if}
  </div>
</header>

<style>
  .bar {
    height: 46px;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    padding: 0 14px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .logo {
    width: 22px;
    height: 22px;
    border-radius: 6px;
    display: grid;
    place-items: center;
    color: #fff;
    background: var(--accent-grad);
  }
  .tabs {
    display: flex;
    gap: 2px;
    padding: 3px;
    border-radius: 9px;
    background: var(--panel-2);
    border: 1px solid var(--border);
  }
  .tab {
    border: none;
    background: transparent;
    color: var(--muted);
    padding: 5px 14px;
    border-radius: 6px;
  }
  .tab:hover:not(.active) {
    color: var(--text);
  }
  .tab.active {
    background: var(--panel-3);
    color: var(--text);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
  }
  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }
  .lang {
    width: auto;
    padding: 4px 6px;
    font-size: 12px;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    border-radius: 99px;
    background: var(--panel-3);
    font-size: 12px;
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  .pill.ready .dot {
    background: var(--ok);
  }
  .update {
    color: var(--accent-2);
    font-weight: 600;
  }
  /* Narrow windows (or a large content scale): icon-only tabs, compact status. */
  @media (max-width: 760px) {
    .bar {
      padding: 0 8px;
      gap: 6px;
    }
    .brand .faint,
    .label,
    .pill .faint {
      display: none;
    }
    .tab {
      padding: 5px 10px;
    }
    .pill {
      padding: 3px 7px;
    }
    .right {
      gap: 4px;
    }
  }
  .spin {
    display: inline-flex;
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
