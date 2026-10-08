//! Model management: catalogue merging, install / update / remove / import.
//!
//! Managed models live in `Engine::model_store()/<id>/` with a `model.json`
//! manifest; the engine decides file names through `Engine::model_layout`.
//! Every change is staged in a hidden sibling directory and swapped in with a
//! rename, so a failed or cancelled operation never leaves a half model.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ivsr_core::model::valid_model_id;
use ivsr_core::{
    CancelToken, Catalog, Engine, HardwareProfile, ImageIo, ModelFile, ModelInfo, ModelManifest, ModelOrigin,
    ParamValues, Reference, TaskContext, TaskMode, Text, Throughput, UpscaleTask,
};
use ivsr_update::{Expected, HttpClient};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::engines::InstallProgress;
use crate::{Error, Result};

const MANIFEST: &str = "model.json";
const CATALOG_TTL: Duration = Duration::from_secs(24 * 3600);
/// First line of an ncnn `.param` file.
const NCNN_MAGIC: &str = "7767517";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    /// Ships with the engine.
    Bundled,
    /// Downloaded and identical to the catalogue entry.
    Installed,
    /// Downloaded, but the catalogue now lists different files.
    UpdateAvailable,
    /// Added from local files.
    Imported,
    /// Listed in a catalogue, not installed.
    Available,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub status: ModelStatus,
    /// Present when installed.
    pub installed: Option<ModelInfo>,
    /// Present when listed in a catalogue.
    pub manifest: Option<ModelManifest>,
    /// `built-in` or the catalogue URL.
    pub source: Option<String>,
    /// Fitted from the reference baseline, when there is one.
    pub reference_throughput: Option<Throughput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelOverview {
    pub engine: String,
    pub reference: Reference,
    pub entries: Vec<ModelEntry>,
    /// Hardware profiles by architecture, for entries that are not installed.
    pub architectures: BTreeMap<String, HardwareProfile>,
    /// Advice per model id for the GPU given to `with_advice`.
    pub advice: BTreeMap<String, crate::advice::Advice>,
    /// Catalogue URLs that could not be loaded, with the reason.
    pub catalog_errors: Vec<(String, String)>,
}

impl ModelEntry {
    /// Hardware profile from the installed model or its catalogue architecture.
    pub fn profile<'a>(&'a self, architectures: &'a BTreeMap<String, HardwareProfile>) -> Option<&'a HardwareProfile> {
        self.installed.as_ref().and_then(|i| i.hardware.as_ref()).or_else(|| {
            let arch = self.manifest.as_ref()?.architecture.as_ref()?;
            architectures.get(arch)
        })
    }
}

impl ModelOverview {
    /// Fills `advice` for every entry against `gpu`.
    pub fn with_advice(mut self, gpu: Option<&crate::system::GpuInfo>) -> Self {
        self.advice = self
            .entries
            .iter()
            .map(|e| (e.id.clone(), crate::advice::advise(e.profile(&self.architectures), gpu)))
            .collect();
        self
    }
}

/// A catalogue manifest together with where it came from.
#[derive(Debug, Clone)]
pub(crate) struct Listed {
    manifest: ModelManifest,
    source: String,
}

pub(crate) struct Catalogs {
    pub reference: Reference,
    pub architectures: BTreeMap<String, HardwareProfile>,
    pub models: Vec<Listed>,
    pub errors: Vec<(String, String)>,
}

/// Built-in catalogue first, then remote ones; the first entry for an id wins
/// so a remote catalogue cannot shadow a built-in model.
pub(crate) fn load_catalogs(
    engine: &dyn Engine,
    urls: &[String],
    cache_dir: &Path,
    http: &dyn HttpClient,
    refresh: bool,
) -> Catalogs {
    let id = engine.info().id;
    let builtin = engine.catalog().unwrap_or_default();
    let mut out = Catalogs {
        reference: builtin.reference.clone(),
        architectures: BTreeMap::new(),
        models: Vec::new(),
        errors: Vec::new(),
    };
    let push = |catalog: Catalog, source: &str, out: &mut Catalogs| {
        for (arch, profile) in catalog.architectures {
            out.architectures.entry(arch).or_insert(profile);
        }
        for manifest in catalog.models {
            let usable = manifest.engine == id
                && valid_model_id(&manifest.id)
                && manifest.files.iter().all(|f| f.url.starts_with("https://") && f.sha256.len() == 64);
            if usable && !out.models.iter().any(|l| l.manifest.id == manifest.id) {
                out.models.push(Listed { manifest, source: source.to_string() });
            }
        }
    };
    push(builtin, "built-in", &mut out);
    for url in urls {
        match remote_catalog(url, cache_dir, http, refresh) {
            Ok(catalog) => push(catalog, url, &mut out),
            Err(e) => out.errors.push((url.clone(), e.to_string())),
        }
    }
    out
}

fn remote_catalog(url: &str, cache_dir: &Path, http: &dyn HttpClient, refresh: bool) -> Result<Catalog> {
    if !url.starts_with("https://") {
        return Err(Error::Input(format!("catalogue URL must use https: {url}")));
    }
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let cached = cache_dir.join(format!("{:016x}.json", hasher.finish()));
    let fresh = fs::metadata(&cached)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().unwrap_or(Duration::MAX) < CATALOG_TTL);
    let read_cache = || -> Option<Catalog> { serde_json::from_slice(&fs::read(&cached).ok()?).ok() };
    if fresh && !refresh
        && let Some(c) = read_cache() {
            return Ok(c);
        }
    match ivsr_update::http::get_json::<Catalog>(http, url, &[]) {
        Ok(catalog) => {
            let _ = fs::create_dir_all(cache_dir);
            let _ = fs::write(&cached, serde_json::to_vec(&catalog).unwrap_or_default());
            Ok(catalog)
        }
        // Offline: an old copy beats nothing.
        Err(e) => read_cache().ok_or(Error::Update(e)),
    }
}

pub(crate) fn overview(engine: &dyn Engine, catalogs: Catalogs) -> ModelOverview {
    let installed = engine.models();
    let mut entries: Vec<ModelEntry> = Vec::new();
    for info in installed {
        let listed = catalogs.models.iter().find(|l| l.manifest.id == info.id);
        let status = match info.origin {
            ModelOrigin::Bundled => ModelStatus::Bundled,
            ModelOrigin::Imported => ModelStatus::Imported,
            ModelOrigin::Catalog => match listed {
                Some(l) if !same_files(&info, &l.manifest) => ModelStatus::UpdateAvailable,
                _ => ModelStatus::Installed,
            },
        };
        let baseline = if info.baseline.is_empty() { listed.map(|l| l.manifest.baseline.clone()).unwrap_or_default() } else { info.baseline.clone() };
        entries.push(ModelEntry {
            id: info.id.clone(),
            status,
            manifest: listed.map(|l| l.manifest.clone()),
            source: listed.map(|l| l.source.clone()),
            reference_throughput: Throughput::fit(&baseline),
            installed: Some(info),
        });
    }
    for listed in &catalogs.models {
        if listed.manifest.bundled || entries.iter().any(|e| e.id == listed.manifest.id) {
            continue;
        }
        entries.push(ModelEntry {
            id: listed.manifest.id.clone(),
            status: ModelStatus::Available,
            installed: None,
            reference_throughput: Throughput::fit(&listed.manifest.baseline),
            manifest: Some(listed.manifest.clone()),
            source: Some(listed.source.clone()),
        });
    }
    ModelOverview {
        engine: engine.info().id,
        reference: catalogs.reference,
        entries,
        architectures: catalogs.architectures,
        advice: BTreeMap::new(),
        catalog_errors: catalogs.errors,
    }
}

/// An update exists when the catalogue's file hashes differ from the installed ones.
fn same_files(info: &ModelInfo, manifest: &ModelManifest) -> bool {
    manifest.files.iter().all(|f| info.file_hashes.iter().any(|(role, sha)| role == &f.role && sha.eq_ignore_ascii_case(&f.sha256)))
}

fn store_of(engine: &dyn Engine) -> Result<PathBuf> {
    engine
        .model_store()
        .ok_or_else(|| Error::Input(format!("engine `{}` does not manage models", engine.info().id)))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn io(action: &str, path: &Path, e: std::io::Error) -> Error {
    ivsr_core::Error::io_at(action, path, e).into()
}

/// Replaces `<store>/<id>` with the fully prepared `staging` directory.
fn swap_in(store: &Path, id: &str, staging: &Path) -> Result<PathBuf> {
    let target = store.join(id);
    let old = store.join(format!(".old-{id}"));
    if old.exists() {
        fs::remove_dir_all(&old).map_err(|e| io("clear", &old, e))?;
    }
    if target.exists() {
        fs::rename(&target, &old).map_err(|e| io("move aside", &target, e))?;
    }
    if let Err(e) = fs::rename(staging, &target) {
        let _ = fs::rename(&old, &target);
        return Err(io("install", &target, e));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(target)
}

fn fresh_staging(store: &Path, id: &str) -> Result<PathBuf> {
    let staging = store.join(format!(".staging-{id}"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| io("clear", &staging, e))?;
    }
    fs::create_dir_all(&staging).map_err(|e| io("create", &staging, e))?;
    Ok(staging)
}

fn write_manifest(dir: &Path, manifest: &ModelManifest) -> Result<()> {
    let path = dir.join(MANIFEST);
    let json = serde_json::to_vec_pretty(manifest).expect("manifest serializes");
    fs::write(&path, json).map_err(|e| io("write", &path, e))
}

pub(crate) fn install(
    engine: &dyn Engine,
    catalogs: &Catalogs,
    id: &str,
    http: Arc<dyn HttpClient>,
    progress: &mut dyn FnMut(InstallProgress),
    cancel: &CancelToken,
) -> Result<ModelInfo> {
    progress(InstallProgress::Resolving);
    let listed = catalogs
        .models
        .iter()
        .find(|l| l.manifest.id == id)
        .ok_or_else(|| Error::Input(format!("model `{id}` is not in any catalogue")))?;
    let manifest = &listed.manifest;
    if manifest.bundled || manifest.files.is_empty() {
        return Err(Error::Input(format!("model `{id}` ships with the engine; reinstall the engine instead")));
    }
    let layout = engine.model_layout(manifest)?;
    let store = store_of(engine)?;
    let staging = fresh_staging(&store, id)?;

    let total: u64 = manifest.download_size();
    let mut done = 0u64;
    let result = (|| -> Result<()> {
        for file in &manifest.files {
            let rel = layout
                .iter()
                .find(|(role, _)| role == &file.role)
                .map(|(_, p)| p.clone())
                .ok_or_else(|| Error::Input(format!("engine has no place for `{}` files", file.role)))?;
            let target = staging.join(&rel);
            let dir = target.parent().unwrap_or(&staging).to_path_buf();
            let name = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let expected = Expected { size: file.size, digest: Some(&file.sha256) };
            let base = done;
            ivsr_update::download_url(
                http.as_ref(),
                &file.url,
                &[],
                &name,
                &expected,
                &dir,
                &mut |received, _| progress(InstallProgress::Downloading { received: base + received, total: Some(total) }),
                &|| cancel.is_cancelled(),
            )?;
            done += file.size;
        }
        progress(InstallProgress::Extracting);
        let mut installed = manifest.clone();
        installed.origin = ModelOrigin::Catalog;
        installed.installed_at = Some(now());
        write_manifest(&staging, &installed)
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    swap_in(&store, id, &staging)?;
    progress(InstallProgress::Done);
    find_installed(engine, id)
}

fn find_installed(engine: &dyn Engine, id: &str) -> Result<ModelInfo> {
    engine
        .models()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| Error::Input(format!("model `{id}` was installed but the engine does not list it")))
}

pub(crate) fn remove(engine: &dyn Engine, id: &str) -> Result<()> {
    let info = engine
        .models()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| Error::Input(format!("model `{id}` is not installed")))?;
    if !info.removable {
        return Err(Error::Input(format!("model `{id}` ships with the engine and cannot be removed on its own")));
    }
    let store = store_of(engine)?;
    if !valid_model_id(id) {
        return Err(Error::Input(format!("invalid model id `{id}`")));
    }
    let dir = store.join(id);
    fs::remove_dir_all(&dir).map_err(|e| io("remove", &dir, e))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImportRequest {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub scale: u32,
    pub param: PathBuf,
    pub bin: PathBuf,
    pub license: Option<String>,
}

fn sha256_of(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|e| io("open", path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| io("read", path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Architecture guessed from the weight count (fp16 bytes / 2), for hardware advice.
fn guess_architecture(bin_size: u64) -> &'static str {
    match bin_size {
        s if s > 20_000_000 => "rrdb",
        s if s > 5_000_000 => "rrdb-6b",
        _ => "compact",
    }
}

pub(crate) fn import(
    engine: &dyn Engine,
    images: &dyn ImageIo,
    req: &ImportRequest,
    defaults: &ParamValues,
    work_root: &Path,
) -> Result<ModelInfo> {
    let id = req.id.trim();
    if !valid_model_id(id) {
        return Err(Error::Input(format!("invalid model id `{id}` (use letters, digits, `-`, `_`, `.`)")));
    }
    if engine.models().iter().any(|m| m.id == id) {
        return Err(Error::Input(format!("a model named `{id}` is already installed")));
    }
    for path in [&req.param, &req.bin] {
        if !path.is_file() {
            return Err(Error::Input(format!("{} does not exist", path.display())));
        }
    }
    let first_line = fs::File::open(&req.param)
        .ok()
        .and_then(|f| BufReader::new(f).lines().next())
        .and_then(|l| l.ok())
        .unwrap_or_default();
    if first_line.trim() != NCNN_MAGIC {
        return Err(Error::Input(format!("{} is not an ncnn .param file", req.param.display())));
    }
    let bin_size = fs::metadata(&req.bin).map(|m| m.len()).unwrap_or(0);
    let file = |role: &str, path: &Path| -> Result<ModelFile> {
        Ok(ModelFile {
            role: role.into(),
            url: format!("file://{}", path.display()),
            size: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            sha256: sha256_of(path)?,
        })
    };
    let manifest = ModelManifest {
        id: id.to_string(),
        engine: engine.info().id,
        name: req.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.to_string()),
        description: req.description.clone().map(Text::en).unwrap_or_default(),
        version: String::new(),
        scales: vec![req.scale],
        tags: vec!["custom".into()],
        architecture: Some(guess_architecture(bin_size).into()),
        license: req.license.clone(),
        author: None,
        homepage: None,
        files: vec![file("param", &req.param)?, file("bin", &req.bin)?],
        bundled: false,
        baseline: Vec::new(),
        origin: ModelOrigin::Imported,
        installed_at: Some(now()),
    };
    let layout = engine.model_layout(&manifest)?;
    let store = store_of(engine)?;
    let staging = fresh_staging(&store, id)?;
    let staged = (|| -> Result<()> {
        for (role, rel) in &layout {
            let src = if role == "param" { &req.param } else { &req.bin };
            let dst = staging.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| io("create", parent, e))?;
            }
            fs::copy(src, &dst).map_err(|e| io("copy", src, e))?;
        }
        write_manifest(&staging, &manifest)
    })();
    if let Err(e) = staged {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    let target = swap_in(&store, id, &staging)?;
    // Prove the files load and produce the declared scale before keeping them.
    if let Err(e) = verify(engine, images, id, req.scale, defaults, work_root) {
        let _ = fs::remove_dir_all(&target);
        return Err(Error::Input(format!("model `{id}` failed verification: {e}")));
    }
    find_installed(engine, id)
}

const PROBE_SIDE: u32 = 16;

/// Upscales a small test image and checks the output is exactly `scale` times larger.
fn verify(
    engine: &dyn Engine,
    images: &dyn ImageIo,
    id: &str,
    scale: u32,
    params: &ParamValues,
    work_root: &Path,
) -> Result<()> {
    fs::create_dir_all(work_root).map_err(|e| io("create", work_root, e))?;
    let dir = tempfile::Builder::new().prefix("ivsr-verify-").tempdir_in(work_root).map_err(|e| io("create", work_root, e))?;
    let input = dir.path().join("probe.png");
    ivsr_media::testpattern::write_png(&input, PROBE_SIDE, PROBE_SIDE)?;
    let output = dir.path().join("out.png");
    let progress = |_: f64| {};
    let log = |_: ivsr_core::LogLevel, _: &str| {};
    let task = UpscaleTask { input: &input, output: &output, mode: TaskMode::File, model: id, scale, params };
    engine.upscale(&task, &TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log })?;
    let info = images.probe(&output)?;
    let expected = PROBE_SIDE * scale;
    if (info.width, info.height) != (expected, expected) {
        return Err(Error::Input(format!(
            "declared x{scale}, but produced {}x{} from {PROBE_SIDE}x{PROBE_SIDE} (x{:.2})",
            info.width,
            info.height,
            info.width as f64 / PROBE_SIDE as f64
        )));
    }
    Ok(())
}
