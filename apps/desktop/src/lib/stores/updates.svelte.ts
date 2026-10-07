import { errorText, ipc } from "../ipc";
import type { UpdateView } from "../types";

class UpdateStore {
  view = $state.raw<UpdateView | null>(null);
  checking = $state(false);
  dismissed = $state(false);
  download = $state<{ received: number; total: number | null } | null>(null);
  installer = $state<string | null>(null);
  error = $state<string | null>(null);

  available = $derived(this.view?.status === "available" ? this.view : null);

  async check(force: boolean) {
    this.checking = true;
    try {
      this.view = await ipc.checkUpdate(force);
      this.error = null;
      if (force) this.dismissed = false;
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.checking = false;
    }
  }

  async fetch() {
    this.download = { received: 0, total: null };
    this.error = null;
    try {
      this.installer = await ipc.downloadUpdate((received, total) => (this.download = { received, total }));
      await ipc.openUpdate(this.installer);
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.download = null;
    }
  }

  async skip() {
    const v = this.available;
    if (!v) return;
    await ipc.skipUpdate(v.latest);
    this.dismissed = true;
  }
}

export const updates = new UpdateStore();
