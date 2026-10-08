// The user's current processing choices. Edits persist to the shared
// config.toml (debounced), so the CLI and the app share defaults.

import { app, normalizeSteps } from "./app.svelte";
import type { AudioMode, Config, ConflictPolicy, FilterChain, FilterStage, FilterStep, JobRequest, ParamValue } from "../types";

const SAVE_DELAY_MS = 500;

class SettingsStore {
  engine = $state("realesrgan");
  model = $state<string | null>(null);
  scale = $state(4);
  params = $state<Record<string, ParamValue>>({});
  imageFormat = $state("same");
  imageQuality = $state(92);
  videoCodec = $state("h264");
  videoQuality = $state<number | null>(null);
  videoPreset = $state<string | null>(null);
  audio = $state<AudioMode>("auto");
  container = $state("same");
  outputDir = $state<string | null>(null);
  suffix = $state("_x{scale}");
  conflict = $state<ConflictPolicy>("rename");
  /** Pre- and post-processing chains; `steps: null` follows the built-in order. */
  filters = $state<Record<FilterStage, FilterChain>>({
    pre: { enabled: false, steps: null },
    post: { enabled: false, steps: null },
  });

  #loaded = false;
  #timer: ReturnType<typeof setTimeout> | undefined;

  engineView = $derived(app.engine(this.engine));
  models = $derived(this.engineView?.models ?? []);
  currentModel = $derived(this.models.find((m) => m.id === this.model) ?? this.models[0]);
  codec = $derived(app.codecs.find((c) => c.id === this.videoCodec));

  constructor() {
    $effect.root(() => {
      $effect(() => {
        const snapshot = this.snapshot();
        if (!this.#loaded) return;
        clearTimeout(this.#timer);
        this.#timer = setTimeout(() => this.#persist(snapshot), SAVE_DELAY_MS);
      });
    });
  }

  /** Adopts the persisted configuration (call after bootstrap). */
  load(config: Config) {
    this.#loaded = false;
    const engineCfg = config.engines[config.engine];
    this.engine = config.engine;
    this.model = engineCfg?.model ?? app.engine(config.engine)?.default_model ?? null;
    this.params = { ...(engineCfg?.params ?? {}) };
    this.scale = config.output.scale;
    this.imageFormat = config.output.image_format;
    this.imageQuality = config.output.image_quality;
    this.suffix = config.output.suffix;
    this.conflict = config.output.conflict;
    this.outputDir = config.output.directory ?? null;
    this.videoCodec = config.video.codec;
    this.videoQuality = config.video.quality ?? null;
    this.videoPreset = config.video.preset ?? null;
    this.audio = config.video.audio;
    this.container = config.video.container;
    const chain = (c: FilterChain | undefined): FilterChain => ({
      enabled: c?.enabled ?? false,
      steps: c?.steps ? normalizeSteps(c.steps) : null,
    });
    this.filters = { pre: chain(config.filters?.pre), post: chain(config.filters?.post) };
    queueMicrotask(() => (this.#loaded = true));
  }

  /** Steps `stage` runs when on: the user's own, or the built-in order. */
  steps(stage: FilterStage): FilterStep[] {
    return this.filters[stage].steps ?? app.defaultSteps(stage);
  }

  /** `null` restores the built-in order. */
  setSteps(stage: FilterStage, steps: FilterStep[] | null) {
    this.filters[stage] = { ...this.filters[stage], steps };
  }

  setFiltersEnabled(stage: FilterStage, enabled: boolean) {
    this.filters[stage] = { ...this.filters[stage], enabled };
  }

  /** Engine parameter value, falling back to the schema default. */
  param(key: string): ParamValue | undefined {
    return this.params[key] ?? this.engineView?.params.find((p) => p.key === key)?.default;
  }

  setParam(key: string, value: ParamValue) {
    this.params = { ...this.params, [key]: value };
  }

  resetParams() {
    this.params = {};
  }

  request(): JobRequest {
    return {
      engine: this.engine,
      model: this.currentModel?.id,
      scale: this.scale,
      params: $state.snapshot(this.params),
      image_format: this.imageFormat,
      image_quality: this.imageQuality,
      video_codec: this.videoCodec,
      video_quality: this.videoQuality,
      video_preset: this.videoPreset,
      audio: this.audio,
      container: this.container,
      output: this.outputDir,
      suffix: this.suffix,
      conflict: this.conflict,
      pre: $state.snapshot(this.filters.pre),
      post: $state.snapshot(this.filters.post),
    };
  }

  /** Plain copy of every persisted field; reading it subscribes to all of them. */
  snapshot() {
    return $state.snapshot({
      engine: this.engine,
      model: this.model,
      scale: this.scale,
      params: this.params,
      imageFormat: this.imageFormat,
      imageQuality: this.imageQuality,
      videoCodec: this.videoCodec,
      videoQuality: this.videoQuality,
      videoPreset: this.videoPreset,
      audio: this.audio,
      container: this.container,
      outputDir: this.outputDir,
      suffix: this.suffix,
      conflict: this.conflict,
      filters: this.filters,
    });
  }

  async #persist(s: ReturnType<SettingsStore["snapshot"]>) {
    try {
      await app.update((config) => {
        config.engine = s.engine;
        const engineCfg = (config.engines[s.engine] ??= { params: {} });
        engineCfg.model = s.model;
        engineCfg.params = s.params;
        Object.assign(config.output, {
          scale: s.scale,
          image_format: s.imageFormat,
          image_quality: s.imageQuality,
          suffix: s.suffix,
          conflict: s.conflict,
          directory: s.outputDir,
        });
        config.filters = s.filters;
        Object.assign(config.video, {
          codec: s.videoCodec,
          quality: s.videoQuality,
          preset: s.videoPreset,
          audio: s.audio,
          container: s.container,
        });
      });
    } catch (e) {
      console.error("failed to save settings", e);
    }
  }
}

export const settings = new SettingsStore();
