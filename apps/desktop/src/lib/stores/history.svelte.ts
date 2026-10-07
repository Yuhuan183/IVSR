// Finished jobs (from the CLI too) for the browse view.

import { errorText, ipc } from "../ipc";
import type { HistoryItem, MediaKind } from "../types";

class HistoryStore {
  items = $state.raw<HistoryItem[]>([]);
  filter = $state<"all" | MediaKind>("all");
  query = $state("");
  loading = $state(false);
  error = $state<string | null>(null);
  /** Set when jobs finish; the browse view reloads lazily. */
  stale = $state(true);

  visible = $derived.by(() => {
    const q = this.query.trim().toLowerCase();
    return this.items.filter(
      (i) =>
        (this.filter === "all" || i.kind === this.filter) &&
        (!q || i.output.toLowerCase().includes(q) || i.model.toLowerCase().includes(q)),
    );
  });

  async load() {
    this.loading = true;
    try {
      this.items = await ipc.history();
      this.error = null;
      this.stale = false;
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.loading = false;
    }
  }

  async remove(ids: number[]) {
    await ipc.removeHistory(ids);
    this.items = this.items.filter((i) => !ids.includes(i.id));
  }

  async clear() {
    await ipc.clearHistory();
    this.items = [];
  }
}

export const history = new HistoryStore();
