<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import Icon from "./Icon.svelte";

  let { onfiles, onfolder }: { onfiles: () => void; onfolder: () => void } = $props();
</script>

<div class="empty">
  <div class="badge"><Icon name="upload" size={28} /></div>
  <h2>{t("empty.title")}</h2>
  <p class="muted">{t("empty.body")}</p>
  <div class="buttons">
    <button onclick={onfiles}><Icon name="plus" /> {t("empty.add_files")}</button>
    <button onclick={onfolder}><Icon name="folder" /> {t("empty.add_folder")}</button>
  </div>
</div>

<style>
  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border: 1.5px dashed var(--border-strong);
    border-radius: 14px;
    text-align: center;
    padding: 32px;
  }
  .badge {
    width: 64px;
    height: 64px;
    border-radius: 18px;
    display: grid;
    place-items: center;
    color: #fff;
    background: var(--accent-grad);
    margin-bottom: 8px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  p {
    margin: 0;
    max-width: 380px;
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 14px;
  }
</style>
