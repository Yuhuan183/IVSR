// Static-ish application data from the core: engines, formats, codecs, config.

import { i18n } from "../i18n/index.svelte";
import { errorText, ipc } from "../ipc";
import type { Bootstrap, Config, EngineView, FilterStage, FilterStep, FilterView } from "../types";

class AppStore {
  boot = $state.raw<Bootstrap | null>(null);
  error = $state<string | null>(null);

  engines = $derived(this.boot?.engines ?? []);
  imageFormats = $derived((this.boot?.formats ?? []).filter((f) => f.kind === "image"));
  videoFormats = $derived((this.boot?.formats ?? []).filter((f) => f.kind === "video"));
  codecs = $derived(this.boot?.codecs ?? []);
  videoReady = $derived(this.boot?.video.state === "ready");
  filters = $derived(this.boot?.filters ?? []);

  /** Extensions accepted by the file picker. */
  inputExtensions = $derived(
    (this.boot?.formats ?? []).filter((f) => f.decode).flatMap((f) => f.extensions),
  );

  engine(id: string): EngineView | undefined {
    return this.engines.find((e) => e.info.id === id);
  }

  filter(id: string): FilterView | undefined {
    return this.filters.find((f) => f.info.id === id);
  }

  /** Built-in steps of `stage`, for chains that do not list their own. */
  defaultSteps(stage: FilterStage): FilterStep[] {
    return normalizeSteps(this.boot?.filter_defaults[stage] ?? []);
  }

  async load() {
    try {
      this.boot = await ipc.bootstrap();
      i18n.locale = this.boot.language;
      this.error = null;
    } catch (e) {
      this.error = errorText(e);
    }
  }

  #saving: Promise<unknown> = Promise.resolve();

  /** Saves `edit` applied to the latest configuration. Saves run one at a
   * time, each on top of the previous result, so stores that persist
   * different sections never overwrite one another. */
  update(edit: (config: Config) => void): Promise<void> {
    const next = this.#saving.then(async () => {
      if (!this.boot) return;
      const config = structuredClone($state.snapshot(this.boot.config)) as Config;
      edit(config);
      this.boot = await ipc.saveConfig(config);
      i18n.locale = this.boot.language;
    });
    this.#saving = next.catch(() => {});
    return next;
  }

  /** `auto`, `en` or `zh-TW`; persisted so the CLI follows too. */
  async setLanguage(language: string) {
    await this.update((config) => {
      config.ui = { ...config.ui, language };
    });
  }
}

export const app = new AppStore();

/** The core omits empty `params`; give every step a map to edit. */
export function normalizeSteps(steps: FilterStep[]): FilterStep[] {
  return steps.map((s) => ({ id: s.id, enabled: s.enabled ?? true, params: { ...(s.params ?? {}) } }));
}
