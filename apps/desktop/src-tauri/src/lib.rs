//! Tauri shell: a thin IPC adapter over `ivsr-service`. All media work,
//! file access and process management stay in the core service; the webview
//! only renders state and sends intents.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use ivsr_core::{CancelToken, CodecInfo, FormatInfo, MediaKind, ToolStatus};
use ivsr_service::{
    BenchmarkRecord, Config, EngineView, Flavor, HistoryEntry, ImportRequest, InstallProgress, InstallReport, JobEvent,
    JobId, JobQueue, JobRequest, ModelOverview, Service, SystemInfo, VERSION,
};
use ivsr_update::{Asset, UpdateCheck};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, RunEvent, State};
use tauri_plugin_opener::OpenerExt;

type CmdResult<T> = Result<T, String>;

const THUMBNAIL_SIDE: u32 = 160;

struct AppState {
    service: RwLock<Arc<Service>>,
    queue: JobQueue,
    subscriber: Arc<Mutex<Option<Channel<JobEvent>>>>,
    /// Asset of the update offered by the last check.
    update: Mutex<Option<Asset>>,
    /// Installers downloaded this session; the only files `open_update` may launch.
    downloads: Mutex<HashSet<PathBuf>>,
    /// Files passed on the command line ("Open with ivsr"), handed to the UI once.
    launch_inputs: Mutex<Vec<PathBuf>>,
}

impl AppState {
    fn service(&self) -> Arc<Service> {
        self.service.read().unwrap().clone()
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Runs blocking core work off the IPC thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

#[derive(Serialize)]
struct Bootstrap {
    version: &'static str,
    platform: &'static str,
    /// Resolved interface language (`en` / `zh-TW`).
    language: &'static str,
    benchmarks: Vec<BenchmarkRecord>,
    engines: Vec<EngineView>,
    formats: Vec<FormatInfo>,
    codecs: Vec<CodecInfo>,
    video: ToolStatus,
    config: Config,
    config_file: PathBuf,
    update_configured: bool,
}

fn bootstrap_of(service: &Service) -> Bootstrap {
    let registry = service.registry();
    Bootstrap {
        version: VERSION,
        platform: std::env::consts::OS,
        language: service.lang().tag(),
        benchmarks: service.benchmarks(),
        engines: service.engine_views(),
        formats: registry.formats(),
        codecs: registry.codecs(),
        video: registry.video().status(),
        config: service.config().clone(),
        config_file: service.config_file().to_path_buf(),
        update_configured: service.updates(Flavor::Desktop, VERSION).is_configured(),
    }
}

#[tauri::command]
async fn bootstrap(state: State<'_, AppState>) -> CmdResult<Bootstrap> {
    let service = state.service();
    blocking(move || Ok(bootstrap_of(&service))).await
}

#[tauri::command]
async fn save_config(state: State<'_, AppState>, config: Config) -> CmdResult<Bootstrap> {
    apply_config(&state, config).await
}

#[tauri::command]
fn take_launch_inputs(state: State<'_, AppState>) -> Vec<PathBuf> {
    std::mem::take(&mut *state.launch_inputs.lock().unwrap())
}

#[tauri::command]
fn subscribe_jobs(state: State<'_, AppState>, channel: Channel<JobEvent>) {
    *state.subscriber.lock().unwrap() = Some(channel);
}

#[derive(Serialize)]
struct InputItem {
    path: PathBuf,
    name: String,
    kind: MediaKind,
    size: u64,
    width: Option<u32>,
    height: Option<u32>,
    frames: Option<u64>,
    duration: Option<f64>,
    error: Option<String>,
}

fn inspect(service: &Service, path: PathBuf) -> Option<InputItem> {
    let registry = service.registry();
    let (kind, _) = registry.classify(&path)?;
    let mut item = InputItem {
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        kind,
        width: None,
        height: None,
        frames: None,
        duration: None,
        error: None,
        path,
    };
    match kind {
        MediaKind::Image => match registry.images().probe(&item.path) {
            Ok(info) => (item.width, item.height) = (Some(info.width), Some(info.height)),
            Err(e) => item.error = Some(e.to_string()),
        },
        MediaKind::Video => match registry.video().probe(&item.path) {
            Ok(info) => {
                (item.width, item.height) = (Some(info.width), Some(info.height));
                item.frames = Some(info.estimated_frames());
                item.duration = info.duration;
            }
            Err(e) => item.error = Some(e.to_string()),
        },
    }
    Some(item)
}

#[tauri::command]
async fn inspect_inputs(app: AppHandle, state: State<'_, AppState>, paths: Vec<PathBuf>, recursive: bool) -> CmdResult<Vec<InputItem>> {
    let service = state.service();
    blocking(move || {
        let files = service.expand_inputs(&paths, recursive).map_err(err)?;
        let items: Vec<InputItem> = files.into_iter().filter_map(|p| inspect(&service, p)).collect();
        // Originals are shown by the viewer next to their results.
        let scope = app.asset_protocol_scope();
        for item in &items {
            let _ = scope.allow_file(&item.path);
        }
        Ok(items)
    })
    .await
}

#[tauri::command]
async fn thumbnail(state: State<'_, AppState>, path: PathBuf) -> CmdResult<PathBuf> {
    let service = state.service();
    blocking(move || service.thumbnail(&path, THUMBNAIL_SIDE).map_err(err)).await
}

#[derive(Serialize)]
struct Enqueued {
    input: PathBuf,
    output: PathBuf,
    kind: MediaKind,
    id: Option<JobId>,
    skip: Option<ivsr_service::SkipReason>,
}

#[tauri::command]
async fn enqueue(state: State<'_, AppState>, inputs: Vec<PathBuf>, request: JobRequest) -> CmdResult<Vec<Enqueued>> {
    let service = state.service();
    let svc = service.clone();
    let prepared = blocking(move || svc.prepare(&inputs, &request).map_err(err)).await?;
    Ok(prepared
        .jobs
        .iter()
        .map(|job| {
            let id = prepared.spec(job).map(|spec| state.queue.submit(service.clone(), prepared.engine.clone(), spec));
            Enqueued { input: job.input.clone(), output: job.output.clone(), kind: job.kind, id, skip: job.skip.clone() }
        })
        .collect())
}

#[tauri::command]
fn cancel_job(state: State<'_, AppState>, id: JobId) -> bool {
    state.queue.cancel(id)
}

#[tauri::command]
fn cancel_all(state: State<'_, AppState>) {
    state.queue.cancel_all();
}

#[tauri::command]
async fn install_engine(state: State<'_, AppState>, engine: String, channel: Channel<InstallProgress>) -> CmdResult<InstallReport> {
    let service = state.service();
    blocking(move || {
        let mut last = Instant::now() - Duration::from_secs(1);
        service
            .install_engine(
                &engine,
                &mut |p| {
                    let is_download = matches!(p, InstallProgress::Downloading { .. });
                    if !is_download || last.elapsed() >= Duration::from_millis(100) {
                        last = Instant::now();
                        let _ = channel.send(p);
                    }
                },
                &CancelToken::new(),
            )
            .map_err(err)
    })
    .await
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum UpdateView {
    NotConfigured,
    UpToDate { current: String, latest: Option<String> },
    Available { current: String, latest: String, notes: String, page_url: Option<String>, asset: Option<AssetView> },
}

#[derive(Serialize)]
struct AssetView {
    name: String,
    size: u64,
}

/// `force` queries now; otherwise only when the auto-check interval elapsed
/// or an earlier check already found an update.
#[tauri::command]
async fn check_update(state: State<'_, AppState>, force: bool) -> CmdResult<UpdateView> {
    let service = state.service();
    let check = blocking(move || {
        let updates = service.updates(Flavor::Desktop, VERSION);
        if !updates.is_configured() {
            return Ok(None);
        }
        let result = if force || updates.known_update().is_some() {
            Some(updates.check().map_err(err)?)
        } else {
            updates.check_if_due()
        };
        Ok(Some(result))
    })
    .await?;
    let view = match check {
        None => UpdateView::NotConfigured,
        Some(None) => UpdateView::UpToDate { current: VERSION.into(), latest: None },
        Some(Some(UpdateCheck::UpToDate { current, latest })) => {
            UpdateView::UpToDate { current: current.to_string(), latest: latest.map(|v| v.to_string()) }
        }
        Some(Some(UpdateCheck::Available { current, latest, release, asset })) => {
            *state.update.lock().unwrap() = asset.clone();
            UpdateView::Available {
                current: current.to_string(),
                latest: latest.to_string(),
                notes: release.notes,
                page_url: release.page_url,
                asset: asset.map(|a| AssetView { name: a.name, size: a.size }),
            }
        }
    };
    Ok(view)
}

#[derive(Clone, Serialize)]
struct DownloadProgress {
    received: u64,
    total: Option<u64>,
}

#[tauri::command]
async fn download_update(state: State<'_, AppState>, channel: Channel<DownloadProgress>) -> CmdResult<PathBuf> {
    let asset = state.update.lock().unwrap().clone().ok_or("no update has been offered")?;
    let service = state.service();
    let path = blocking(move || {
        let mut last = Instant::now() - Duration::from_secs(1);
        service
            .updates(Flavor::Desktop, VERSION)
            .download(
                &asset,
                &mut |received, total| {
                    if last.elapsed() >= Duration::from_millis(100) || Some(received) == total {
                        last = Instant::now();
                        let _ = channel.send(DownloadProgress { received, total });
                    }
                },
                &CancelToken::new(),
            )
            .map(|d| d.path)
            .map_err(err)
    })
    .await?;
    state.downloads.lock().unwrap().insert(path.clone());
    Ok(path)
}

/// Launches a downloaded installer with the platform handler.
#[tauri::command]
fn open_update(app: AppHandle, state: State<'_, AppState>, path: PathBuf) -> CmdResult<()> {
    if !state.downloads.lock().unwrap().contains(&path) {
        return Err("not a downloaded update".into());
    }
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
fn skip_update(state: State<'_, AppState>, version: String) -> CmdResult<()> {
    let version = ivsr_update::release::parse_tag(&version).ok_or("invalid version")?;
    state.service().updates(Flavor::Desktop, VERSION).skip(&version).map_err(err)
}

#[tauri::command]
fn reveal(app: AppHandle, path: PathBuf) -> CmdResult<()> {
    if !path.exists() {
        return Err(format!("{} no longer exists", path.display()));
    }
    app.opener().reveal_item_in_dir(&path).map_err(err)
}

/// Persists `config` and swaps the live service; shared by commands that edit settings.
async fn apply_config(state: &State<'_, AppState>, config: Config) -> CmdResult<Bootstrap> {
    let service = state.service();
    let updated = Arc::new(blocking(move || service.reconfigure(config).map_err(err)).await?);
    *state.service.write().unwrap() = updated.clone();
    blocking(move || Ok(bootstrap_of(&updated))).await
}

/// Overview with advice for the GPU the configured parameters select.
#[tauri::command]
async fn model_overview(state: State<'_, AppState>, engine: String, refresh: bool) -> CmdResult<ModelOverview> {
    let service = state.service();
    blocking(move || {
        let overview = service.model_overview(&engine, refresh).map_err(err)?;
        let gpu = service.system_info(&engine).ok().and_then(|s| service.active_gpu(&engine, &s));
        Ok(overview.with_advice(gpu.as_ref()))
    })
    .await
}

#[tauri::command]
async fn install_model(
    state: State<'_, AppState>,
    engine: String,
    model: String,
    channel: Channel<InstallProgress>,
) -> CmdResult<Bootstrap> {
    let service = state.service();
    let svc = service.clone();
    blocking(move || {
        let mut last = Instant::now() - Duration::from_secs(1);
        svc.install_model(
            &engine,
            &model,
            &mut |p| {
                let is_download = matches!(p, InstallProgress::Downloading { .. });
                if !is_download || last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    let _ = channel.send(p);
                }
            },
            &CancelToken::new(),
        )
        .map_err(err)
    })
    .await?;
    blocking(move || Ok(bootstrap_of(&service))).await
}

#[tauri::command]
async fn remove_model(state: State<'_, AppState>, engine: String, model: String) -> CmdResult<Bootstrap> {
    if state.queue.pending() > 0 {
        return Err("busy".into());
    }
    let service = state.service();
    let changed = blocking({
        let service = service.clone();
        move || service.remove_model(&engine, &model).map_err(err)
    })
    .await?;
    match changed {
        Some(config) => apply_config(&state, config).await,
        None => blocking(move || Ok(bootstrap_of(&service))).await,
    }
}

#[tauri::command]
async fn import_model(state: State<'_, AppState>, engine: String, request: ImportRequest) -> CmdResult<Bootstrap> {
    if state.queue.pending() > 0 {
        return Err("busy".into());
    }
    let service = state.service();
    let svc = service.clone();
    blocking(move || svc.import_model(&engine, &request).map(|_| ()).map_err(err)).await?;
    blocking(move || Ok(bootstrap_of(&service))).await
}

/// Benchmarks contend with real jobs for the GPU, so they only run when idle.
#[tauri::command]
async fn benchmark_model(
    state: State<'_, AppState>,
    engine: String,
    model: String,
    channel: Channel<f64>,
) -> CmdResult<BenchmarkRecord> {
    if state.queue.pending() > 0 {
        return Err("busy".into());
    }
    let service = state.service();
    blocking(move || {
        service
            .benchmark(&engine, &model, None, &mut |f| {
                let _ = channel.send(f);
            }, &CancelToken::new())
            .map_err(err)
    })
    .await
}

#[derive(Serialize)]
struct SystemView {
    system: SystemInfo,
    /// GPU the configured parameters select.
    active: Option<ivsr_service::GpuInfo>,
}

#[tauri::command]
async fn system_info(state: State<'_, AppState>, engine: String) -> CmdResult<SystemView> {
    let service = state.service();
    blocking(move || {
        let system = service.system_info(&engine).map_err(err)?;
        let active = service.active_gpu(&engine, &system);
        Ok(SystemView { system, active })
    })
    .await
}

#[derive(Serialize)]
struct HistoryItem {
    #[serde(flatten)]
    entry: HistoryEntry,
    input_exists: bool,
    output_exists: bool,
}

/// History for the browse view; grants the webview read access to each pair.
#[tauri::command]
async fn history(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Vec<HistoryItem>> {
    let service = state.service();
    blocking(move || {
        let scope = app.asset_protocol_scope();
        Ok(service
            .history()
            .into_iter()
            .map(|entry| {
                let input_exists = entry.input.is_file();
                let output_exists = entry.output.is_file();
                if input_exists {
                    let _ = scope.allow_file(&entry.input);
                }
                if output_exists {
                    let _ = scope.allow_file(&entry.output);
                }
                HistoryItem { entry, input_exists, output_exists }
            })
            .collect())
    })
    .await
}

#[tauri::command]
async fn remove_history(state: State<'_, AppState>, ids: Vec<u64>) -> CmdResult<()> {
    let service = state.service();
    blocking(move || service.remove_history(&ids).map_err(err)).await
}

#[tauri::command]
async fn clear_history(state: State<'_, AppState>) -> CmdResult<()> {
    let service = state.service();
    blocking(move || service.clear_history().map_err(err)).await
}

fn allow_output(app: &AppHandle, path: &Path) {
    let _ = app.asset_protocol_scope().allow_file(path);
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let service = Service::load(None)?;
            let thumbs = service.thumbnail_dir();
            std::fs::create_dir_all(&thumbs)?;
            app.asset_protocol_scope().allow_directory(&thumbs, false)?;

            let subscriber: Arc<Mutex<Option<Channel<JobEvent>>>> = Arc::default();
            let sink = subscriber.clone();
            let handle = app.handle().clone();
            let queue = JobQueue::new(Arc::new(move |event| {
                if let JobEvent::Completed { outcome, .. } = &event {
                    allow_output(&handle, &outcome.output);
                }
                if let Some(channel) = sink.lock().unwrap().as_ref() {
                    let _ = channel.send(event);
                }
            }));
            app.manage(AppState {
                service: RwLock::new(Arc::new(service)),
                queue,
                subscriber,
                update: Mutex::new(None),
                downloads: Mutex::new(HashSet::new()),
                launch_inputs: Mutex::new(std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_config,
            subscribe_jobs,
            take_launch_inputs,
            inspect_inputs,
            thumbnail,
            enqueue,
            cancel_job,
            cancel_all,
            install_engine,
            check_update,
            download_update,
            open_update,
            skip_update,
            reveal,
            model_overview,
            install_model,
            remove_model,
            import_model,
            benchmark_model,
            system_info,
            history,
            remove_history,
            clear_history,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build ivsr");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Cancel running work so engine and ffmpeg processes are killed and
            // temporary files removed before the process ends.
            let state = handle.state::<AppState>();
            state.queue.cancel_all();
            let deadline = Instant::now() + Duration::from_secs(3);
            while state.queue.pending() > 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    });
}
