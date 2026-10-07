// The before/after viewer, opened from the queue or the browse view.

import type { MediaKind } from "../types";

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

class ViewerStore {
  list = $state.raw<ViewerItem[]>([]);
  index = $state(-1);
  mode = $state<CompareMode>("slider");

  current = $derived(this.index >= 0 ? this.list[this.index] : null);

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
