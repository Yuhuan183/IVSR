// Static-ish application data from the core: engines, formats, codecs, config.

import { i18n } from "../i18n/index.svelte";
import { errorText, ipc } from "../ipc";
import type { Bootstrap, Config, EngineView } from "../types";

class AppStore {
  boot = $state.raw<Bootstrap | null>(null);
  error = $state<string | null>(null);

  engines = $derived(this.boot?.engines ?? []);
  imageFormats = $derived((this.boot?.formats ?? []).filter((f) => f.kind === "image"));
  videoFormats = $derived((this.boot?.formats ?? []).filter((f) => f.kind === "video"));
  codecs = $derived(this.boot?.codecs ?? []);
  videoReady = $derived(this.boot?.video.state === "ready");

  /** Extensions accepted by the file picker. */
  inputExtensions = $derived(
    (this.boot?.formats ?? []).filter((f) => f.decode).flatMap((f) => f.extensions),
  );

  engine(id: string): EngineView | undefined {
    return this.engines.find((e) => e.info.id === id);
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

  async save(config: Config) {
    this.boot = await ipc.saveConfig(config);
    i18n.locale = this.boot.language;
  }

  /** `auto`, `en` or `zh-TW`; persisted so the CLI follows too. */
  async setLanguage(language: string) {
    if (!this.boot) return;
    const config = structuredClone($state.snapshot(this.boot.config)) as Config;
    config.ui = { ...config.ui, language };
    await this.save(config);
  }
}

export const app = new AppStore();
