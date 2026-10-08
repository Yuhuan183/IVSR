// The before/after viewer, opened from the queue or the browse view.

import type { FilterStage, FilterStep, MediaKind } from "../types";

export interface ViewerItem {
  kind: MediaKind;
  name: string;
  original: string;
  result: string;
  sourceSize: [number, number] | null;
  resultSize: [number, number] | null;
  model?: string;
  scale?: number;
  elapsedMs?: number | null;
}

export type CompareMode = "slider" | "split" | "fade";

/** Which picture the filter panel processes: before or after super-resolution. */
export type FilterTarget = "result" | "original";

/** What the filtered picture is compared against: the same picture
 * unfiltered, or the other side of the super-resolution. */
export type FilterBase = "unfiltered" | "other";

class ViewerStore {
  list = $state.raw<ViewerItem[]>([]);
  index = $state(-1);
  mode = $state<CompareMode>("slider");

  current = $derived(this.index >= 0 ? this.list[this.index] : null);

  /** Filter panel. Steps are kept per stage while the app runs. */
  panel = $state(false);
  /** Master switch: whether the filtered picture is shown at all. */
  applied = $state(true);
  target = $state<FilterTarget>("result");
  base = $state<FilterBase>("unfiltered");
  steps = $state<Record<FilterStage, FilterStep[] | null>>({ pre: null, post: null });
  /** Preview file for the current item, target and steps. */
  processed = $state<string | null>(null);
  /** Enabled steps the preview was asked for. */
  activeSteps = $state(0);
  busy = $state(false);
  error = $state<string | null>(null);

  stage = $derived<FilterStage>(this.target === "result" ? "post" : "pre");
  /** Whether the stage shows a filtered picture (or is computing one). */
  filtering = $derived(this.panel && this.applied && this.current?.kind === "image");

  /** `\`: flips the master switch, opening the panel when it is closed. */
  toggleApplied() {
    if (!this.panel) {
      this.panel = true;
      this.applied = true;
    } else {
      this.applied = !this.applied;
    }
  }

  open(item: ViewerItem, list: ViewerItem[] = [item]) {
    this.list = list;
    this.index = Math.max(0, list.indexOf(item));
  }

  close() {
    this.index = -1;
  }

  step(delta: number) {
    if (this.list.length === 0) return;
    this.index = (this.index + delta + this.list.length) % this.list.length;
  }
}

export const viewer = new ViewerStore();
