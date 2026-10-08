// App updates. Checks (automatic at start-up, or from the title bar) only
// announce a new version; installing always goes through the confirmation
// dialog, which downloads the update and then applies it: an AppImage is
// replaced and the app restarts, anything else opens the platform installer.

import { t } from "../i18n/index.svelte";
import { app } from "./app.svelte";
import { errorText, ipc } from "../ipc";
import type { UpdateView } from "../types";

/** How long the "up to date" note after a manual check stays visible. */
const NOTE_MS = 4000;

export type InstallPhase = "confirm" | "downloading" | "opened" | "restarting" | "failed";

class UpdateStore {
  view = $state.raw<UpdateView | null>(null);
  checking = $state(false);
  /** The banner was closed for this session; the title bar badge stays. */
  dismissed = $state(false);
  /** A manual check found nothing new. */
  upToDate = $state(false);
  /** The last manual check failed (automatic checks fail quietly). */
  error = $state<string | null>(null);

  /** The confirmation dialog, when open. */
  phase = $state<InstallPhase | null>(null);
  download = $state<{ received: number; total: number | null } | null>(null);
  installError = $state<string | null>(null);

  available = $derived(this.view?.status === "available" ? this.view : null);
  mode = $derived(app.boot?.update_mode ?? "installer");

  #noteTimer: ReturnType<typeof setTimeout> | undefined;

  async check(force: boolean) {
    this.checking = true;
    try {
      this.view = await ipc.checkUpdate(force);
      this.error = null;
      if (force) {
        this.dismissed = false;
        this.upToDate = this.view.status === "up_to_date";
        clearTimeout(this.#noteTimer);
        if (this.upToDate) this.#noteTimer = setTimeout(() => (this.upToDate = false), NOTE_MS);
      }
    } catch (e) {
      if (force) {
        this.error = errorText(e);
        clearTimeout(this.#noteTimer);
        this.#noteTimer = setTimeout(() => (this.error = null), NOTE_MS * 2);
      }
    } finally {
      this.checking = false;
    }
  }

  /** Opens the dialog that asks before anything is downloaded. */
  ask() {
    if (!this.available) return;
    this.installError = null;
    this.phase = "confirm";
  }

  /** The user agreed: download, verify and apply. */
  async install() {
    this.phase = "downloading";
    this.download = { received: 0, total: null };
    this.installError = null;
    try {
      const file = await ipc.downloadUpdate((received, total) => (this.download = { received, total }));
      if (this.mode === "appimage") this.phase = "restarting";
      await ipc.installUpdate(file);
      if (this.mode !== "appimage") this.phase = "opened";
    } catch (e) {
      const message = errorText(e);
      this.installError = message === "busy" ? t("update.busy") : message;
      this.phase = "failed";
    } finally {
      this.download = null;
    }
  }

  close() {
    if (this.phase !== "downloading" && this.phase !== "restarting") this.phase = null;
  }

  async skip() {
    const v = this.available;
    if (!v) return;
    try {
      await ipc.skipUpdate(v.latest);
      this.view = null;
      this.phase = null;
    } catch (e) {
      this.installError = errorText(e);
    }
  }
}

export const updates = new UpdateStore();
