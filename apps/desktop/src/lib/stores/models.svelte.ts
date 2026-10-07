// Model management view state and performance estimates.

import { errorText, ipc } from "../ipc";
import type { ImportRequest, ModelOverview, SystemView, Throughput } from "../types";
import { app } from "./app.svelte";

export type Busy = { kind: "install" | "remove" | "bench"; progress: number };

class ModelStore {
  overview = $state.raw<ModelOverview | null>(null);
  system = $state.raw<SystemView | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  busy = $state<Record<string, Busy>>({});

  engine = $derived(app.boot?.config.engine ?? "realesrgan");
  activeGpu = $derived(this.system?.active ?? this.system?.system.gpus[0] ?? null);

  async load(refresh = false) {
    this.loading = true;
    try {
      this.overview = await ipc.modelOverview(this.engine, refresh);
      this.error = null;
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.loading = false;
    }
  }

  async loadSystem() {
    try {
      this.system = await ipc.systemInfo(this.engine);
    } catch {
      // Hardware details are advisory.
    }
  }

  async #run(id: string, kind: Busy["kind"], action: () => Promise<void>) {
    this.busy = { ...this.busy, [id]: { kind, progress: 0 } };
    try {
      await action();
      this.error = null;
    } catch (e) {
      this.error = errorText(e);
    } finally {
      const { [id]: _, ...rest } = this.busy;
      this.busy = rest;
    }
  }

  #progress(id: string, progress: number) {
    const b = this.busy[id];
    if (b) this.busy = { ...this.busy, [id]: { ...b, progress } };
  }

  install(id: string) {
    return this.#run(id, "install", async () => {
      app.boot = await ipc.installModel(this.engine, id, (p) => {
        if (p.phase === "downloading" && p.total) this.#progress(id, p.received / p.total);
      });
      await this.load();
    });
  }

  remove(id: string) {
    return this.#run(id, "remove", async () => {
      app.boot = await ipc.removeModel(this.engine, id);
      await this.load();
    });
  }

  bench(id: string) {
    return this.#run(id, "bench", async () => {
      const record = await ipc.benchmarkModel(this.engine, id, (f) => this.#progress(id, f));
      if (app.boot) {
        const others = app.boot.benchmarks.filter(
          (b) => !(b.engine === record.engine && b.model === record.model && b.scale === record.scale && b.device === record.device),
        );
        app.boot = { ...app.boot, benchmarks: [...others, record] };
      }
    });
  }

  async importModel(request: ImportRequest) {
    app.boot = await ipc.importModel(this.engine, request);
    await this.load();
  }

  /** Local benchmark on the active GPU, else the reference baseline when it was measured on this GPU. */
  throughput(model: string, scale: number): { value: Throughput; local: boolean } | null {
    const gpu = this.activeGpu?.name;
    const local = app.boot?.benchmarks.find(
      (b) => b.model === model && b.scale === scale && b.engine === this.engine && (!gpu || b.device === gpu),
    );
    if (local) return { value: local.throughput, local: true };
    const entry = this.overview?.entries.find((e) => e.id === model);
    const reference = this.overview?.reference.device ?? "";
    const scales = entry?.installed?.scales ?? entry?.manifest?.scales ?? [];
    if (entry?.reference_throughput && gpu && reference.startsWith(gpu) && scales[0] === scale) {
      return { value: entry.reference_throughput, local: false };
    }
    return null;
  }
}

export const models = new ModelStore();
