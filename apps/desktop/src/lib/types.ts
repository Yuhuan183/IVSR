// Mirrors of the serde shapes exchanged with the Rust core (ivsr-service).

export type MediaKind = "image" | "video";
export type Stage = "preparing" | "decoding" | "upscaling" | "encoding" | "finalizing";
export type AudioMode = "auto" | "copy" | "reencode" | "drop";
export type ConflictPolicy = "rename" | "overwrite" | "skip";
export type Channel = "stable" | "beta";

/** Localized text from the core: `{ en, "zh-TW", ... }`. */
export type Text = Record<string, string>;

export type ToolStatus =
  | { state: "ready"; location: string; version: string | null }
  | { state: "missing"; hint: string }
  | { state: "broken"; reason: string };

export type ParamValue = boolean | number | string;

export type ParamKind =
  | { type: "bool" }
  | { type: "int"; min: number | null; max: number | null }
  | { type: "float"; min: number | null; max: number | null }
  | { type: "enum"; options: { value: string; label: Text }[] }
  | { type: "text" };

export interface ParamSpec {
  key: string;
  label: Text;
  description: Text;
  kind: ParamKind;
  default: ParamValue;
  advanced: boolean;
}

export type CostClass = "light" | "medium" | "heavy";
export type ModelOrigin = "bundled" | "catalog" | "imported";

export interface BaselinePoint {
  width: number;
  height: number;
  seconds: number;
}

export interface Throughput {
  startup: number;
  per_megapixel: number;
}

export interface HardwareProfile {
  class: CostClass;
  summary: Text;
  memory_by_tile: { tile: number; mb: number }[];
}

export interface ModelInfo {
  id: string;
  name: string;
  description: Text;
  scales: number[];
  tags: string[];
  origin: ModelOrigin;
  removable: boolean;
  version: string | null;
  license: string | null;
  author: string | null;
  homepage: string | null;
  size: number;
  architecture: string | null;
  parameters: number | null;
  class: CostClass | null;
  hardware: HardwareProfile | null;
  baseline: BaselinePoint[];
}

export interface ModelManifest {
  id: string;
  engine: string;
  name: string;
  description: Text;
  version: string;
  scales: number[];
  tags: string[];
  architecture: string | null;
  license: string | null;
  author: string | null;
  homepage: string | null;
  files: { role: string; url: string; size: number; sha256: string }[];
  bundled: boolean;
  baseline: BaselinePoint[];
}

export type ModelStatus = "bundled" | "installed" | "update_available" | "imported" | "available";

export interface ModelEntry {
  id: string;
  status: ModelStatus;
  installed: ModelInfo | null;
  manifest: ModelManifest | null;
  source: string | null;
  reference_throughput: Throughput | null;
}

export interface Advice {
  fit: "comfortable" | "constrained" | "insufficient" | "unknown";
  suggested_tile: number | null;
  needed_mb: number | null;
  available_mb: number | null;
}

export interface ModelOverview {
  engine: string;
  reference: { device: string; runtime: string; measured_on: string };
  entries: ModelEntry[];
  architectures: Record<string, HardwareProfile>;
  advice: Record<string, Advice>;
  catalog_errors: [string, string][];
}

export interface BenchmarkRecord {
  engine: string;
  model: string;
  scale: number;
  device: string;
  points: BaselinePoint[];
  throughput: Throughput;
  measured_at: number;
}

export interface GpuInfo {
  index: number | null;
  name: string;
  memory_mb: number | null;
  unified: boolean;
}

export interface SystemView {
  system: {
    os: string;
    arch: string;
    cpu: string | null;
    threads: number;
    memory_mb: number | null;
    gpus: GpuInfo[];
  };
  active: GpuInfo | null;
}

export interface ImportRequest {
  id: string;
  name: string | null;
  description: string | null;
  scale: number;
  param: string;
  bin: string;
  license: string | null;
}

export interface HistoryItem {
  id: number;
  input: string;
  output: string;
  kind: MediaKind;
  engine: string;
  model: string;
  scale: number;
  source_width: number;
  source_height: number;
  width: number;
  height: number;
  frames: number | null;
  elapsed_ms: number;
  input_exists: boolean;
  output_exists: boolean;
}

export type SkipReason =
  | { code: "unsupported" }
  | { code: "output_exists" }
  | { code: "video_unavailable"; detail: string };

export interface EngineView {
  info: { id: string; name: string; description: Text; homepage: string | null };
  status: ToolStatus;
  models: ModelInfo[];
  default_model: string | null;
  params: ParamSpec[];
  caps: { input_formats: string[]; output_format: string; batch: boolean };
  installable: boolean;
  installed: { release: string; asset: string; verification: "sha256" | "size_only"; installed_at: number } | null;
}

export interface FormatInfo {
  id: string;
  label: string;
  kind: MediaKind;
  extensions: string[];
  decode: boolean;
  encode: boolean;
  lossy: boolean;
  note?: Text | null;
}

export interface CodecInfo {
  id: string;
  label: string;
  containers: string[];
  quality: { label: string; min: number; max: number; default: number } | null;
  presets: string[];
  default_preset: string | null;
  hardware: boolean;
  available: boolean;
}

export interface EngineConfig {
  path?: string | null;
  models_dir?: string | null;
  model?: string | null;
  params: Record<string, ParamValue>;
}

export interface Config {
  engine: string;
  work_dir?: string | null;
  output: {
    scale: number;
    image_format: string;
    image_quality: number;
    suffix: string;
    directory?: string | null;
    conflict: ConflictPolicy;
  };
  video: {
    codec: string;
    quality?: number | null;
    preset?: string | null;
    audio: AudioMode;
    container: string;
    batch_frames: number;
  };
  tools: { ffmpeg?: string | null; ffprobe?: string | null };
  engines: Record<string, EngineConfig>;
  ui: { language: string };
  models: { catalogs: string[] };
  history: { enabled: boolean; limit: number };
  update: {
    provider: string;
    repository: string;
    channel: Channel;
    auto_check: boolean;
    interval_hours: number;
    api_base?: string | null;
    token_env: string;
  };
}

export interface Bootstrap {
  version: string;
  platform: string;
  language: "en" | "zh-TW";
  benchmarks: BenchmarkRecord[];
  engines: EngineView[];
  formats: FormatInfo[];
  codecs: CodecInfo[];
  video: ToolStatus;
  config: Config;
  config_file: string;
  update_configured: boolean;
}

export interface InputItem {
  path: string;
  name: string;
  kind: MediaKind;
  size: number;
  width: number | null;
  height: number | null;
  frames: number | null;
  duration: number | null;
  error: string | null;
}

export interface JobRequest {
  engine?: string;
  model?: string;
  scale?: number;
  params?: Record<string, ParamValue>;
  image_format?: string;
  image_quality?: number;
  video_codec?: string;
  video_quality?: number | null;
  video_preset?: string | null;
  audio?: AudioMode;
  container?: string;
  output?: string | null;
  suffix?: string;
  conflict?: ConflictPolicy;
}

export interface Enqueued {
  input: string;
  output: string;
  kind: MediaKind;
  id: number | null;
  skip: SkipReason | null;
}

export interface JobOutcome {
  output: string;
  width: number;
  height: number;
  source_width: number;
  source_height: number;
  frames?: number;
}

export type JobEvent =
  | { type: "started"; id: number }
  | { type: "progress"; id: number; stage: Stage; overall: number; units?: [number, number] }
  | { type: "log"; id: number; level: "info" | "warn"; message: string }
  | { type: "completed"; id: number; outcome: JobOutcome; elapsed_ms: number }
  | { type: "failed"; id: number; error: string }
  | { type: "cancelled"; id: number };

export type InstallProgress =
  | { phase: "resolving" }
  | { phase: "downloading"; received: number; total: number | null }
  | { phase: "extracting" }
  | { phase: "done" };

export type UpdateView =
  | { status: "not_configured" }
  | { status: "up_to_date"; current: string; latest: string | null }
  | {
      status: "available";
      current: string;
      latest: string;
      notes: string;
      page_url: string | null;
      asset: { name: string; size: number } | null;
    };
