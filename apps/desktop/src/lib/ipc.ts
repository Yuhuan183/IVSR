// Typed wrappers around the Tauri commands in src-tauri/src/lib.rs.

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  BenchmarkRecord,
  Bootstrap,
  Config,
  Enqueued,
  FilterOutcome,
  FilterStage,
  FilterStep,
  HistoryItem,
  ImportRequest,
  InputItem,
  InstallProgress,
  JobEvent,
  JobRequest,
  ModelOverview,
  SystemView,
  UpdateView,
} from "./types";

export const ipc = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  saveConfig: (config: Config) => invoke<Bootstrap>("save_config", { config }),

  subscribeJobs(onEvent: (event: JobEvent) => void) {
    const channel = new Channel<JobEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("subscribe_jobs", { channel });
  },
  takeLaunchInputs: () => invoke<string[]>("take_launch_inputs"),
  inspectInputs: (paths: string[], recursive: boolean) =>
    invoke<InputItem[]>("inspect_inputs", { paths, recursive }),
  thumbnail: (path: string) => invoke<string>("thumbnail", { path }),
  filterPreview: (path: string, reference: string | null, stage: FilterStage, steps: FilterStep[]) =>
    invoke<string>("filter_preview", { path, reference, stage, steps }),
  filterSave: (path: string, reference: string | null, stage: FilterStage, steps: FilterStep[], output: string) =>
    invoke<FilterOutcome>("filter_save", { path, reference, stage, steps, output }),
  enqueue: (inputs: string[], request: JobRequest) => invoke<Enqueued[]>("enqueue", { inputs, request }),
  cancelJob: (id: number) => invoke<boolean>("cancel_job", { id }),
  cancelAll: () => invoke<void>("cancel_all"),

  installEngine(engine: string, onProgress: (p: InstallProgress) => void) {
    const channel = new Channel<InstallProgress>();
    channel.onmessage = onProgress;
    return invoke<unknown>("install_engine", { engine, channel });
  },

  checkUpdate: (force: boolean) => invoke<UpdateView>("check_update", { force }),
  downloadUpdate(onProgress: (received: number, total: number | null) => void) {
    const channel = new Channel<{ received: number; total: number | null }>();
    channel.onmessage = (p) => onProgress(p.received, p.total);
    return invoke<string>("download_update", { channel });
  },
  installUpdate: (path: string) => invoke<void>("install_update", { path }),
  skipUpdate: (version: string) => invoke<void>("skip_update", { version }),
  reveal: (path: string) => invoke<void>("reveal", { path }),

  modelOverview: (engine: string, refresh: boolean) => invoke<ModelOverview>("model_overview", { engine, refresh }),
  installModel(engine: string, model: string, onProgress: (p: InstallProgress) => void) {
    const channel = new Channel<InstallProgress>();
    channel.onmessage = onProgress;
    return invoke<Bootstrap>("install_model", { engine, model, channel });
  },
  removeModel: (engine: string, model: string) => invoke<Bootstrap>("remove_model", { engine, model }),
  importModel: (engine: string, request: ImportRequest) => invoke<Bootstrap>("import_model", { engine, request }),
  benchmarkModel(engine: string, model: string, onProgress: (fraction: number) => void) {
    const channel = new Channel<number>();
    channel.onmessage = onProgress;
    return invoke<BenchmarkRecord>("benchmark_model", { engine, model, channel });
  },
  systemInfo: (engine: string) => invoke<SystemView>("system_info", { engine }),
  history: () => invoke<HistoryItem[]>("history"),
  removeHistory: (ids: number[]) => invoke<void>("remove_history", { ids }),
  clearHistory: () => invoke<void>("clear_history"),
};

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
