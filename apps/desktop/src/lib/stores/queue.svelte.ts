// Queue items. Each item is its own reactive object, so a progress event
// re-renders only that row's progress bar.

import { errorText, ipc } from "../ipc";
import type { InputItem, JobEvent, JobRequest, MediaKind, SkipReason, Stage } from "../types";
import { history } from "./history.svelte";

export type Status = "idle" | "queued" | "running" | "done" | "failed" | "cancelled" | "skipped";

export class QueueItem {
  readonly path: string;
  readonly name: string;
  readonly kind: MediaKind;
  readonly size: number;
  readonly width: number | null;
  readonly height: number | null;
  readonly frames: number | null;
  readonly duration: number | null;

  thumb = $state<string | null>(null);
  status = $state<Status>("idle");
  progress = $state(0);
  stage = $state<Stage | null>(null);
  units = $state<[number, number] | null>(null);
  output = $state<string | null>(null);
  outSize = $state<[number, number] | null>(null);
  message = $state<string | null>(null);
  skip = $state<SkipReason | null>(null);
  elapsed = $state<number | null>(null);
  jobId: number | null = null;

  constructor(input: InputItem) {
    this.path = input.path;
    this.name = input.name;
    this.kind = input.kind;
    this.size = input.size;
    this.width = input.width;
    this.height = input.height;
    this.frames = input.frames;
    this.duration = input.duration;
    if (input.error) {
      this.status = "skipped";
      this.message = input.error;
    }
  }

  get active() {
    return this.status === "queued" || this.status === "running";
  }

  get pending() {
    return this.status === "idle" || this.status === "failed" || this.status === "cancelled";
  }
}

class QueueStore {
  items = $state<QueueItem[]>([]);
  error = $state<string | null>(null);
  adding = $state(false);
  #byJob = new Map<number, QueueItem>();
  /** Events for jobs whose id has not come back from `enqueue` yet: the
   * worker may start (and even finish) a job before that response arrives. */
  #early = new Map<number, JobEvent[]>();

  counts = $derived.by(() => {
    const c = { total: 0, pending: 0, active: 0, done: 0, failed: 0 };
    for (const i of this.items) {
      c.total++;
      if (i.pending) c.pending++;
      if (i.active) c.active++;
      if (i.status === "done") c.done++;
      if (i.status === "failed") c.failed++;
    }
    return c;
  });

  async init() {
    await ipc.subscribeJobs((e) => this.#onEvent(e));
  }

  async add(paths: string[], recursive = true) {
    if (paths.length === 0) return;
    this.adding = true;
    try {
      const known = new Set(this.items.map((i) => i.path));
      const found = await ipc.inspectInputs(paths, recursive);
      const fresh = found.filter((f) => !known.has(f.path)).map((f) => new QueueItem(f));
      this.items.push(...fresh);
      this.error = found.length === 0 ? "No supported images or videos found." : null;
      void this.#thumbnails(fresh);
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.adding = false;
    }
  }

  /** Generates thumbnails two at a time so large batches stay responsive. */
  async #thumbnails(items: QueueItem[]) {
    const images = items.filter((i) => i.kind === "image" && i.status !== "skipped");
    const worker = async () => {
      for (let item = images.shift(); item; item = images.shift()) {
        try {
          item.thumb = await ipc.thumbnail(item.path);
        } catch {
          // Thumbnails are cosmetic.
        }
      }
    };
    await Promise.all([worker(), worker()]);
  }

  remove(item: QueueItem) {
    if (item.active) return;
    this.items = this.items.filter((i) => i !== item);
  }

  clearFinished() {
    this.items = this.items.filter((i) => i.active || i.status === "idle");
  }

  clearAll() {
    this.items = this.items.filter((i) => i.active);
  }

  async start(request: JobRequest) {
    const ready = this.items.filter((i) => i.pending);
    if (ready.length === 0) return;
    for (const item of ready) {
      item.status = "queued";
      item.progress = 0;
      item.stage = null;
      item.units = null;
      item.message = null;
      item.skip = null;
    }
    try {
      const jobs = await ipc.enqueue(
        ready.map((i) => i.path),
        request,
      );
      const byPath = new Map(ready.map((i) => [i.path, i]));
      for (const job of jobs) {
        const item = byPath.get(job.input);
        if (!item) continue;
        item.output = job.output;
        if (job.id === null) {
          item.status = "skipped";
          item.skip = job.skip;
        } else {
          item.jobId = job.id;
          this.#byJob.set(job.id, item);
          const early = this.#early.get(job.id);
          this.#early.delete(job.id);
          early?.forEach((e) => this.#onEvent(e));
        }
      }
      this.error = null;
    } catch (e) {
      for (const item of ready) item.status = "idle";
      this.error = errorText(e);
    }
  }

  async cancel(item: QueueItem) {
    if (item.jobId !== null) await ipc.cancelJob(item.jobId);
  }

  async cancelAll() {
    await ipc.cancelAll();
  }

  #onEvent(e: JobEvent) {
    const item = this.#byJob.get(e.id);
    if (!item) {
      const pending = this.#early.get(e.id) ?? [];
      // Only lifecycle events and the latest progress matter for replay.
      if (e.type === "progress" && pending.at(-1)?.type === "progress") pending.pop();
      pending.push(e);
      this.#early.set(e.id, pending);
      return;
    }
    switch (e.type) {
      case "started":
        item.status = "running";
        break;
      case "progress":
        item.progress = e.overall;
        item.stage = e.stage;
        item.units = e.units ?? null;
        break;
      case "log":
        item.message = e.message;
        break;
      case "completed":
        item.status = "done";
        item.progress = 1;
        item.output = e.outcome.output;
        item.outSize = [e.outcome.width, e.outcome.height];
        item.elapsed = e.elapsed_ms;
        item.message = null;
        this.#byJob.delete(e.id);
        history.stale = true;
        break;
      case "failed":
        item.status = "failed";
        item.message = e.error;
        this.#byJob.delete(e.id);
        break;
      case "cancelled":
        item.status = "cancelled";
        item.message = null;
        this.#byJob.delete(e.id);
        break;
    }
  }
}

export const queue = new QueueStore();
