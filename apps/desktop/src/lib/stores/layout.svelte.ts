// Content scale (page zoom) and panel widths. They live in the shared
// config.toml next to the language, and are saved once a change settles.

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { app } from "./app.svelte";
import type { Config } from "../types";

export const SCALES = [1, 1.25, 1.5, 1.75, 2];
export const PANEL = { min: 260, max: 640, initial: 340 };
export const FILTER_PANEL = { min: 260, max: 560, initial: 320 };

class LayoutStore {
  scale = $state(1);
  panelWidth = $state(PANEL.initial);
  filterPanelWidth = $state(FILTER_PANEL.initial);

  load(config: Config) {
    const ui = config.ui;
    this.scale = SCALES.includes(ui.scale) ? ui.scale : 1;
    this.panelWidth = ui.panel_width || PANEL.initial;
    this.filterPanelWidth = ui.filter_panel_width || FILTER_PANEL.initial;
  }

  /** Applies the content scale to the window; call from an effect. */
  async applyScale(scale: number) {
    try {
      await getCurrentWebview().setZoom(scale);
    } catch (e) {
      console.error("failed to set content scale", e);
    }
  }

  setScale(scale: number) {
    this.scale = scale;
    void this.persist();
  }

  /** Next or previous preset; `0` restores 100%. */
  stepScale(direction: -1 | 0 | 1) {
    if (direction === 0) return this.setScale(1);
    const i = SCALES.indexOf(this.scale);
    const next = SCALES[Math.min(SCALES.length - 1, Math.max(0, (i < 0 ? 0 : i) + direction))];
    if (next !== this.scale) this.setScale(next);
  }

  async persist() {
    const ui = { scale: this.scale, panel_width: this.panelWidth, filter_panel_width: this.filterPanelWidth };
    try {
      await app.update((config) => {
        config.ui = { ...config.ui, ...ui };
      });
    } catch (e) {
      console.error("failed to save layout", e);
    }
  }
}

export const layout = new LayoutStore();
