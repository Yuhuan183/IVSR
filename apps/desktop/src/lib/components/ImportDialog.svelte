<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { basename } from "../format";
  import { t } from "../i18n/index.svelte";
  import { errorText } from "../ipc";
  import { models } from "../stores/models.svelte";
  import Icon from "./Icon.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let param = $state<string | null>(null);
  let bin = $state<string | null>(null);
  let id = $state("");
  let name = $state("");
  let scale = $state(4);
  let license = $state("");
  let working = $state(false);
  let error = $state<string | null>(null);

  const validId = $derived(/^[A-Za-z0-9_.-]{1,64}$/.test(id) && !id.startsWith("."));
  const ready = $derived(!!param && !!bin && validId && scale >= 1 && scale <= 8);

  async function pick(kind: "param" | "bin") {
    const file = await open({ multiple: false, title: t(kind === "param" ? "import.pick_param" : "import.pick_bin"), filters: [{ name: kind, extensions: [kind] }] });
    if (typeof file !== "string") return;
    if (kind === "param") {
      param = file;
      const stem = basename(file).replace(/\.param$/i, "");
      if (!id) id = stem.replace(/[^A-Za-z0-9_.-]/g, "-");
      if (!bin) bin = file.replace(/\.param$/i, ".bin");
      const hint = /(?:^|[^a-z])x([1-8])(?![0-9])|([1-8])x/i.exec(stem);
      if (hint) scale = Number(hint[1] ?? hint[2]);
    } else {
      bin = file;
    }
  }

  async function submit() {
    if (!ready || !param || !bin) return;
    working = true;
    error = null;
    try {
      await models.importModel({ id, name: name || null, description: null, scale, param, bin, license: license || null });
      onclose();
    } catch (e) {
      const message = errorText(e);
      error = message === "busy" ? t("models.queue_busy") : message;
    } finally {
      working = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !working && onclose()} />

<div class="backdrop" role="presentation" onclick={() => !working && onclose()}>
  <div class="dialog" onclick={(e) => e.stopPropagation()} onkeydown={() => {}} role="dialog" aria-modal="true" aria-label={t("import.title")} tabindex="-1">
  <form class="body" onsubmit={(e) => (e.preventDefault(), submit())}>
    <header>
      <strong>{t("import.title")}</strong>
      <button type="button" class="ghost" onclick={onclose} disabled={working}><Icon name="x" /></button>
    </header>
    <p class="muted">{t("import.intro")}</p>

    <div class="grid">
      <span>{t("import.param")}</span>
      <div class="file">
        <span class="path" title={param ?? ""}>{param ? basename(param) : t("import.none")}</span>
        <button type="button" onclick={() => pick("param")}>{t("import.choose")}</button>
      </div>
      <span>{t("import.bin")}</span>
      <div class="file">
        <span class="path" title={bin ?? ""}>{bin ? basename(bin) : t("import.none")}</span>
        <button type="button" onclick={() => pick("bin")}>{t("import.choose")}</button>
      </div>
      <label for="import-id">{t("import.id")}</label>
      <input id="import-id" type="text" bind:value={id} spellcheck="false" class:invalid={id.length > 0 && !validId} />
      <label for="import-name">{t("import.name")}</label>
      <input id="import-name" type="text" bind:value={name} placeholder={id} />
      <label for="import-scale">{t("import.scale")}</label>
      <input id="import-scale" type="number" min="1" max="8" step="1" bind:value={scale} />
      <label for="import-license">{t("import.license")}</label>
      <input id="import-license" type="text" bind:value={license} placeholder="CC-BY-4.0" spellcheck="false" />
    </div>
    <p class="faint">{t("import.verify_note")}</p>
    {#if error}<p class="err">{error}</p>{/if}

    <footer>
      <button type="button" onclick={onclose} disabled={working}>{t("common.cancel")}</button>
      <button type="submit" class="primary" disabled={!ready || working}>
        <Icon name="upload" /> {working ? t("import.verifying") : t("import.submit")}
      </button>
    </footer>
  </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 25;
    background: rgb(0 0 0 / 0.55);
    display: grid;
    place-items: center;
  }
  .dialog {
    width: min(520px, 92vw);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
    padding: 16px 18px;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  p {
    margin: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 8px 10px;
    align-items: center;
  }
  .file {
    display: flex;
    gap: 8px;
    align-items: center;
    min-width: 0;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
  }
  .invalid {
    border-color: var(--err) !important;
  }
  .err {
    color: var(--err);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
