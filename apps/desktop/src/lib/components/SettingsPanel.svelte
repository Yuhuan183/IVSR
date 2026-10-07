<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { scaleText } from "../format";
  import { t, tx } from "../i18n/index.svelte";
  import { nav } from "../stores/nav.svelte";
  import { app } from "../stores/app.svelte";
  import { settings } from "../stores/settings.svelte";
  import type { AudioMode, ConflictPolicy } from "../types";
  import Icon from "./Icon.svelte";
  import ParamField from "./ParamField.svelte";
  import Section from "./Section.svelte";

  const PRESET_SCALES = [2, 3, 4];

  let showAdvanced = $state(false);

  const engine = $derived(settings.engineView);
  const model = $derived(settings.currentModel);
  const params = $derived(engine?.params ?? []);
  const basicParams = $derived(params.filter((p) => !p.advanced));
  const advancedParams = $derived(params.filter((p) => p.advanced));

  const nativeScale = $derived.by(() => {
    const scales = model?.scales ?? [];
    return scales.find((s) => s >= settings.scale - 1e-6) ?? scales.at(-1);
  });
  const resampled = $derived(nativeScale !== undefined && Math.abs(nativeScale - settings.scale) > 1e-6);

  const writableImages = $derived(app.imageFormats.filter((f) => f.encode));
  const selectedFormat = $derived(writableImages.find((f) => f.id === settings.imageFormat));
  const showQuality = $derived(settings.imageFormat === "same" || selectedFormat?.lossy === true);

  const codec = $derived(settings.codec);
  const containers = $derived(codec?.containers ?? []);

  async function chooseOutput() {
    const dir = await open({ directory: true, multiple: false, title: t("settings.output_folder") });
    if (typeof dir === "string") settings.outputDir = dir;
  }

  function selectCodec(id: string) {
    settings.videoCodec = id;
    settings.videoQuality = null;
    settings.videoPreset = null;
    const next = app.codecs.find((c) => c.id === id);
    if (next && settings.container !== "same" && !next.containers.includes(settings.container)) {
      settings.container = "same";
    }
  }

  function setScale(value: number) {
    if (Number.isFinite(value)) settings.scale = Math.min(16, Math.max(1, value));
  }
</script>

<aside class="panel">
  <Section title={t("settings.model")}>
    {#snippet aside()}
      <button class="ghost small" onclick={() => (nav.view = "models")}>{t("settings.manage_models")}</button>
    {/snippet}
    {#if app.engines.length > 1}
      <select bind:value={settings.engine}>
        {#each app.engines as e (e.info.id)}
          <option value={e.info.id}>{e.info.name}</option>
        {/each}
      </select>
    {/if}
    {#if settings.models.length === 0}
      <p class="faint">{t("settings.no_models")}</p>
    {:else}
      <div class="models" role="radiogroup">
        {#each settings.models as m (m.id)}
          <button
            class="model"
            class:selected={m.id === model?.id}
            role="radio"
            aria-checked={m.id === model?.id}
            onclick={() => (settings.model = m.id)}
          >
            <span class="model-name">{m.name}</span>
            <span class="model-desc">{tx(m.description)}</span>
            <span class="tags">
              {#each m.scales as s (s)}<span class="tag scale">×{s}</span>{/each}
              {#each m.tags as t (t)}<span class="tag">{t}</span>{/each}
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </Section>

  <Section title={t("settings.scale")}>
    <div class="scale-row">
      <div class="segmented">
        {#each PRESET_SCALES as s (s)}
          <button class:on={settings.scale === s} onclick={() => setScale(s)}>×{s}</button>
        {/each}
      </div>
      <input
        type="number"
        min="1"
        max="16"
        step="0.25"
        value={settings.scale}
        onchange={(e) => setScale((e.currentTarget as HTMLInputElement).valueAsNumber)}
        aria-label={t("settings.custom_scale")}
      />
    </div>
    {#if resampled}
      <p class="note">{t("settings.resampled", { native: nativeScale ?? "", scale: scaleText(settings.scale) })}</p>
    {/if}
  </Section>

  {#if params.length > 0}
    <Section title={t("settings.engine")}>
      {#snippet aside()}
        <button class="ghost small" onclick={() => settings.resetParams()}>{t("settings.reset")}</button>
      {/snippet}
      {#each basicParams as spec (spec.key)}
        <ParamField {spec} value={settings.param(spec.key)} onchange={(v) => settings.setParam(spec.key, v)} />
      {/each}
      {#if advancedParams.length > 0}
        <button class="ghost disclosure" onclick={() => (showAdvanced = !showAdvanced)}>
          <span class="chev" class:open={showAdvanced}><Icon name="chevron" size={12} /></span> {t("settings.advanced")}
        </button>
        {#if showAdvanced}
          {#each advancedParams as spec (spec.key)}
            <ParamField {spec} value={settings.param(spec.key)} onchange={(v) => settings.setParam(spec.key, v)} />
          {/each}
        {/if}
      {/if}
    </Section>
  {/if}

  <Section title={t("settings.image_output")}>
    <div class="grid2">
      <label for="image-format">{t("settings.format")}</label>
      <select id="image-format" bind:value={settings.imageFormat}>
        <option value="same">{t("settings.same_as_input")}</option>
        {#each writableImages as f (f.id)}
          <option value={f.id}>{f.label}{f.note ? ` — ${tx(f.note)}` : ""}</option>
        {/each}
      </select>
      {#if showQuality}
        <label for="image-quality">{t("settings.quality")} <span class="faint">{settings.imageQuality}</span></label>
        <input id="image-quality" type="range" min="50" max="100" bind:value={settings.imageQuality} />
      {/if}
    </div>
  </Section>

  <Section title={t("settings.video_output")}>
    {#if !app.videoReady}
      <p class="warn">{t("settings.no_ffmpeg")}</p>
    {/if}
    <div class="grid2">
      <label for="codec">{t("settings.codec")}</label>
      <select id="codec" value={settings.videoCodec} onchange={(e) => selectCodec((e.currentTarget as HTMLSelectElement).value)}>
        {#each app.codecs as c (c.id)}
          <option value={c.id} disabled={!c.available}>{c.label}{c.available ? "" : ` (${t("settings.unavailable")})`}</option>
        {/each}
      </select>

      {#if codec?.quality}
        {@const q = codec.quality}
        <label for="crf">{q.label} <span class="faint">{settings.videoQuality ?? q.default}</span></label>
        <input
          id="crf"
          type="range"
          min={q.min}
          max={q.max}
          value={settings.videoQuality ?? q.default}
          oninput={(e) => (settings.videoQuality = (e.currentTarget as HTMLInputElement).valueAsNumber)}
        />
      {/if}

      {#if codec && codec.presets.length > 0}
        <label for="preset">{t("settings.preset")}</label>
        <select
          id="preset"
          value={settings.videoPreset ?? codec.default_preset ?? ""}
          onchange={(e) => (settings.videoPreset = (e.currentTarget as HTMLSelectElement).value)}
        >
          {#each codec.presets as p (p)}<option value={p}>{p}</option>{/each}
        </select>
      {/if}

      <label for="container">{t("settings.container")}</label>
      <select id="container" bind:value={settings.container}>
        <option value="same">{t("settings.same_as_input")}</option>
        {#each containers as c (c)}<option value={c}>{c.toUpperCase()}</option>{/each}
      </select>

      <label for="audio">{t("settings.audio")}</label>
      <select id="audio" value={settings.audio} onchange={(e) => (settings.audio = (e.currentTarget as HTMLSelectElement).value as AudioMode)}>
        <option value="auto">{t("settings.audio_auto")}</option>
        <option value="copy">{t("settings.audio_copy")}</option>
        <option value="reencode">{t("settings.audio_reencode")}</option>
        <option value="drop">{t("settings.audio_drop")}</option>
      </select>
    </div>
  </Section>

  <Section title={t("settings.save_to")}>
    <div class="output">
      <span class="path" title={settings.outputDir ?? ""}>{settings.outputDir ?? t("settings.next_to_originals")}</span>
      <button onclick={chooseOutput}><Icon name="folder" /> {t("settings.choose")}</button>
      {#if settings.outputDir}
        <button class="ghost" title={t("settings.save_next_to_originals")} onclick={() => (settings.outputDir = null)}><Icon name="x" /></button>
      {/if}
    </div>
    <div class="grid2">
      <label for="suffix">{t("settings.suffix")}</label>
      <input id="suffix" type="text" bind:value={settings.suffix} spellcheck="false" />
      <label for="conflict">{t("settings.conflict")}</label>
      <select id="conflict" value={settings.conflict} onchange={(e) => (settings.conflict = (e.currentTarget as HTMLSelectElement).value as ConflictPolicy)}>
        <option value="rename">{t("settings.conflict_rename")}</option>
        <option value="overwrite">{t("settings.conflict_overwrite")}</option>
        <option value="skip">{t("settings.conflict_skip")}</option>
      </select>
    </div>
  </Section>
</aside>

<style>
  .panel {
    overflow-y: auto;
    background: var(--panel);
    border-left: 1px solid var(--border);
  }
  .models {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .model {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    text-align: left;
    white-space: normal;
    padding: 9px 11px;
    background: var(--panel-2);
    border-color: var(--border);
  }
  .model.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, var(--panel-2));
  }
  .model-name {
    font-weight: 600;
  }
  .model-desc {
    color: var(--muted);
    font-size: 12px;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 2px;
  }
  .tag {
    font-size: 10.5px;
    padding: 1px 6px;
    border-radius: 99px;
    background: var(--panel-3);
    color: var(--muted);
  }
  .tag.scale {
    color: var(--accent-2);
  }
  .scale-row {
    display: grid;
    grid-template-columns: 1fr 76px;
    gap: 8px;
  }
  .segmented {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }
  .segmented button {
    border: none;
    border-radius: 0;
    background: var(--panel-2);
    justify-content: center;
    font-weight: 600;
  }
  .segmented button + button {
    border-left: 1px solid var(--border);
  }
  .segmented button.on {
    background: var(--accent-grad);
    color: #fff;
  }
  .note {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }
  .warn {
    margin: 0;
    font-size: 12px;
    color: var(--warn);
  }
  .grid2 {
    display: grid;
    grid-template-columns: 96px 1fr;
    gap: 8px 10px;
    align-items: center;
  }
  .output {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    color: var(--muted);
  }
  .small {
    font-size: 11.5px;
  }
  .disclosure {
    align-self: flex-start;
  }
  .chev {
    display: inline-flex;
    transition: transform 120ms;
  }
  .chev.open {
    transform: rotate(90deg);
  }
</style>
