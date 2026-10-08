//! CLI message catalogue (English / Traditional Chinese).
//!
//! Messages are `(key, en, zh-TW)` rows; `{name}` placeholders are filled by
//! `tr!`. Help text is translated by walking the clap command tree, so the
//! derive definitions in `cli.rs` stay the English source of truth.

use std::fmt::Display;
use std::sync::OnceLock;

use clap::{Arg, ArgAction, Command};
use ivsr_service::Lang;

static LANG: OnceLock<Lang> = OnceLock::new();

pub fn init(lang: Lang) {
    let _ = LANG.set(lang);
}

pub fn lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::En)
}

/// Translation for `key`, or the key itself when it is missing.
pub fn t(key: &'static str) -> &'static str {
    lookup(lang(), key).unwrap_or(key)
}

fn lookup(lang: Lang, key: &str) -> Option<&'static str> {
    let row = MESSAGES.iter().find(|(k, _, _)| *k == key)?;
    Some(match lang {
        Lang::En => row.1,
        Lang::ZhTw => row.2,
    })
}

/// Replaces `{name}` placeholders.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::t($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::t($key), &[$((stringify!($name), &$value as &dyn std::fmt::Display)),+])
    };
}

/// Engine- and catalogue-supplied text in the current language.
pub fn text(t: &ivsr_core::Text) -> &str {
    t.get(lang().tag())
}

/// Translates help text of `cmd` and its subcommands in place.
pub fn localize(cmd: Command) -> Command {
    if lang() == Lang::En {
        return cmd;
    }
    localize_at(cmd, "")
}

fn help_key(path: &str, id: &str) -> String {
    if path.is_empty() { format!("arg.{id}") } else { format!("arg.{path}.{id}") }
}

fn localize_at(mut cmd: Command, path: &str) -> Command {
    let lang = lang();
    let about_key = if path.is_empty() { "about".to_string() } else { format!("about.{path}") };
    if let Some(about) = lookup(lang, &about_key) {
        cmd = cmd.about(about);
    }
    if path.is_empty() {
        if let Some(after) = lookup(lang, "after_help") {
            cmd = cmd.after_help(after);
        }
        if let Some(usage) = lookup(lang, "usage") {
            cmd = cmd.override_usage(usage);
        }
        cmd = cmd.disable_version_flag(true).arg(
            Arg::new("version").short('V').long("version").action(ArgAction::Version).help(t("arg.version")),
        );
    }
    cmd = cmd
        .disable_help_flag(true)
        .arg(Arg::new("help").short('h').long("help").action(ArgAction::Help).help(t("arg.help")));
    let ids: Vec<String> = cmd.get_arguments().map(|a| a.get_id().to_string()).collect();
    for id in ids {
        if let Some(help) = lookup(lang, &help_key(path, &id)).or_else(|| lookup(lang, &help_key("", &id))) {
            cmd = cmd.mut_arg(&id, |a| a.help(help));
        }
        let heading = cmd
            .get_arguments()
            .find(|a| a.get_id() == id.as_str())
            .and_then(|a| a.get_help_heading())
            .map(str::to_ascii_lowercase);
        if let Some(heading) = heading {
            let key = format!("heading.{heading}");
            if let Some(translated) = MESSAGES.iter().find(|(k, _, _)| *k == key).map(|r| r.2) {
                cmd = cmd.mut_arg(&id, |a| a.help_heading(translated));
            }
        }
    }
    let subs: Vec<String> = cmd.get_subcommands().map(|s| s.get_name().to_string()).collect();
    for name in subs {
        let sub_path = if path.is_empty() { name.clone() } else { format!("{path}.{name}") };
        cmd = cmd.mut_subcommand(&name, |s| localize_at(s, &sub_path));
    }
    cmd
}

/// `(key, en, zh-TW)`.
pub static MESSAGES: &[(&str, &str, &str)] = &[
    // ---- help -------------------------------------------------------------
    ("about", "Image and video super-resolution", "圖片與影片超解析度工具"),
    ("usage", "ivsr [OPTIONS] <INPUT>...   (same as `ivsr upscale`)\n       ivsr [OPTIONS] <COMMAND>", "ivsr [選項] <輸入>...   (等同 `ivsr upscale`)\n       ivsr [選項] <指令>"),
    ("after_help", "", "所有高畫質化選項 (倍率、模型、格式、影片 codec 等): ivsr upscale --help\n\n範例:\n  ivsr photo.jpg                       以 x4 高畫質化, 輸出在原檔旁 (photo_x4.jpg)\n  ivsr -s 2 -f webp shots/ -o out/     shots/ 內所有圖片 x2, 轉 WebP 存到 out/\n  ivsr clip.mp4 --codec h265 --crf 20  影片高畫質化並以 HEVC 重新編碼\n  ivsr --post icon.png                 高畫質化後還原階調與銳利度\n  ivsr filters                         前處理 / 後製濾鏡與執行順序\n  ivsr filters apply out/icon_x4.png   只套用濾鏡, 輸出濾鏡後的圖 (不做高畫質化)\n  ivsr engines install realesrgan      下載 Real-ESRGAN 執行環境\n  ivsr models                          列出已安裝與可下載的模型\n  ivsr models bench --all              在這台電腦上測速, 作為時間預估依據"),
    ("arg.help", "Print help", "顯示說明"),
    ("arg.version", "Print version", "顯示版本"),
    ("heading.video", "Video", "影片"),
    ("heading.filters", "Filters", "前處理 / 後製"),
    ("arg.config", "", "使用指定的設定檔, 而非預設位置"),
    ("arg.json", "", "輸出機器可讀的 JSON (長時間指令每行一個事件)"),
    ("arg.quiet", "", "只顯示錯誤與最終結果"),
    ("arg.verbose", "", "顯示引擎與工具的訊息"),
    ("arg.no_color", "", "停用色彩 (也會遵循 NO_COLOR)"),
    ("arg.lang", "", "介面語言: en、zh-TW (預設依設定檔, 其次依作業系統)"),
    ("about.upscale", "", "圖片與影片高畫質化 (預設指令)"),
    ("arg.upscale.inputs", "", "圖片、影片, 或包含它們的資料夾"),
    ("arg.upscale.output", "", "輸出檔 (單一輸入時) 或資料夾. 預設: 每個輸入檔旁邊"),
    ("arg.upscale.scale", "", "輸出倍率 1-16. 非原生倍率會由模型輸出重新取樣"),
    ("arg.upscale.model", "", "模型 id (見 `ivsr models`)"),
    ("arg.upscale.engine", "", "引擎 id"),
    ("arg.upscale.params", "", "引擎參數, 可重複 (例: -p tile=256 -p tta=true)"),
    ("arg.upscale.format", "", "圖片輸出格式 (png、jpg、webp ...) 或 `same`"),
    ("arg.upscale.quality", "", "有損圖片格式的品質, 1-100"),
    ("arg.upscale.recursive", "", "遞迴搜尋子資料夾, 並在 --output 中保留資料夾結構"),
    ("arg.upscale.suffix", "", "檔名後綴; 可用 {scale}、{model}、{engine}. 預設: _x{scale}"),
    ("arg.upscale.overwrite", "", "覆寫已存在的輸出檔 (預設: 另取不重複的檔名)"),
    ("arg.upscale.skip_existing", "", "輸出檔已存在時跳過該輸入"),
    ("arg.upscale.codec", "", "影片 codec: h264、h265、av1、vp9、prores"),
    ("arg.upscale.crf", "", "影片品質 (CRF); 數字越小品質越好"),
    ("arg.upscale.preset", "", "編碼器 preset (例: medium、slow; AV1 為 4-12)"),
    ("arg.upscale.audio", "", "音軌處理方式"),
    ("arg.upscale.container", "", "輸出容器 (mp4、mkv、mov、webm) 或 `same`"),
    ("arg.upscale.batch_frames", "", "每次引擎執行處理的幀數; 越大越快, 但佔用更多暫存空間"),
    ("arg.upscale.dry_run", "", "只顯示將執行的內容, 不實際處理"),
    ("arg.upscale.pre", "", "高畫質化前先做前處理. 單獨使用: 依設定的步驟; --pre=a,b 依序指定步驟"),
    ("arg.upscale.post", "", "高畫質化後做後製 (階調、銳利度等). 單獨使用: 依設定的步驟; --post=a,b 依序指定步驟"),
    ("arg.upscale.no_pre", "", "這次執行不做前處理"),
    ("arg.upscale.no_post", "", "這次執行不做後製"),
    ("arg.upscale.filter_params", "", "濾鏡參數, 可重複 (例: -F detail-sharpen.amount=1.4). 見 `ivsr filters show <id>`"),
    ("about.filters", "", "列出、檢視與套用前處理 / 後製濾鏡"),
    ("about.filters.list", "", "列出濾鏡與目前設定的處理鏈 (預設)"),
    ("about.filters.show", "", "顯示濾鏡的參數"),
    ("about.filters.apply", "", "直接對圖片套用濾鏡, 不做高畫質化"),
    ("arg.filters.apply.inputs", "", "圖片, 或包含圖片的資料夾"),
    ("arg.filters.apply.output", "", "輸出檔 (單一輸入時) 或資料夾. 預設: 每個輸入檔旁邊"),
    ("arg.filters.apply.stage", "", "要套用哪一段的濾鏡: pre = 原圖 (前處理), post = 高畫質結果 (後製, 以原圖為參考)"),
    ("arg.filters.apply.steps", "", "依序列出濾鏡, 例: tone-restore,detail-sharpen. 預設: 設定中的處理鏈"),
    ("arg.filters.apply.filter_params", "", "濾鏡參數, 可重複 (例: -F detail-sharpen.amount=1.4)"),
    ("arg.filters.apply.reference", "", "輸入檔高畫質化前的原圖, 或以檔名比對的原圖資料夾. 預設: 歷史紀錄中的原圖"),
    ("arg.filters.apply.format", "", "圖片輸出格式 (png、jpg、webp ...) 或 `same`"),
    ("arg.filters.apply.quality", "", "有損圖片格式的品質, 1-100"),
    ("arg.filters.apply.recursive", "", "遞迴搜尋子資料夾, 並在 --output 中保留資料夾結構"),
    ("arg.filters.apply.suffix", "", "檔名後綴. 預設: _post (或 _pre)"),
    ("arg.filters.apply.overwrite", "", "覆寫已存在的輸出檔 (預設: 另取不重複的檔名)"),
    ("arg.filters.apply.skip_existing", "", "輸出檔已存在時跳過該輸入"),
    ("arg.filters.apply.dry_run", "", "只顯示將執行的內容, 不實際處理"),
    ("about.engines", "", "列出、檢視與安裝超解析度引擎"),
    ("about.engines.list", "", "列出引擎與狀態 (預設)"),
    ("about.engines.show", "", "顯示引擎的模型與參數"),
    ("about.engines.install", "", "下載並安裝引擎執行環境"),
    ("arg.engines.install.force", "", "即使引擎已可用也重新安裝"),
    ("about.models", "", "安裝、更新、移除、匯入與測速模型"),
    ("arg.models.engine", "", "要管理哪個引擎的模型 (預設: 設定中的引擎)"),
    ("about.models.list", "", "列出已安裝與可下載的模型 (預設)"),
    ("arg.models.list.refresh", "", "重新下載遠端型錄, 不使用每日快取"),
    ("about.models.show", "", "顯示模型詳情、硬體需求與 baseline 速度"),
    ("about.models.install", "", "從型錄下載模型"),
    ("about.models.update", "", "更新型錄檔案已變更的模型"),
    ("arg.models.update.all", "", "更新所有可更新的模型"),
    ("about.models.remove", "", "刪除已下載或匯入的模型"),
    ("arg.models.remove.yes", "", "不詢問確認"),
    ("about.models.import", "", "從本機 ncnn 檔案 (.param + .bin) 加入模型"),
    ("arg.models.import.id", "", "新模型的 id (英數字與 `-`、`_`、`.`)"),
    ("arg.models.import.param", "", "ncnn .param 檔"),
    ("arg.models.import.bin", "", "ncnn .bin 檔"),
    ("arg.models.import.scale", "", "模型的放大倍率; 匯入時會實際驗證"),
    ("arg.models.import.name", "", "顯示名稱"),
    ("arg.models.import.description", "", "簡短說明"),
    ("arg.models.import.license", "", "授權識別碼, 例如 CC-BY-4.0"),
    ("about.models.use", "", "設為預設的高畫質化模型"),
    ("about.models.bench", "", "在這台電腦上測速模型 (結果用於時間預估與建議)"),
    ("arg.models.bench.all", "", "測速所有已安裝的模型"),
    ("arg.models.bench.scale", "", "要測的倍率 (預設: 模型的第一個原生倍率)"),
    ("about.system", "", "顯示硬體、GPU 與工具狀態"),
    ("about.formats", "", "顯示支援的圖片格式、影片容器與 codec"),
    ("about.probe", "", "顯示媒體檔資訊"),
    ("about.update", "", "檢查並安裝 IVSR 更新"),
    ("about.update.check", "", "檢查是否有新版本 (預設)"),
    ("about.update.install", "", "下載並安裝最新版本"),
    ("arg.update.install.yes", "", "不詢問確認"),
    ("about.update.skip", "", "不再提醒目前可用的版本"),
    ("about.config", "", "顯示或修改設定"),
    ("about.config.show", "", "顯示目前的完整設定 (預設)"),
    ("about.config.path", "", "顯示設定檔與資料的位置"),
    ("about.config.get", "", "顯示單一設定, 例如 `video.codec`"),
    ("about.config.set", "", "修改單一設定, 例如 `ivsr config set output.scale 2`"),
    ("about.config.unset", "", "將單一設定恢復預設"),
    ("about.completions", "", "產生 shell 自動補全腳本"),
    // ---- shared UI ----------------------------------------------------------
    ("ui.error", "error:", "錯誤:"),
    ("ui.warning", "warning:", "警告:"),
    ("ui.hint", "hint:", "提示:"),
    ("ui.yes", "yes", "是"),
    ("ui.no", "no", "否"),
    ("ui.confirm_suffix", "[y/N]", "[y/N]"),
    // ---- upscale ------------------------------------------------------------
    ("upscale.files", "{count} file(s)", "{count} 個檔案"),
    ("upscale.done", "{count} done", "{count} 個完成"),
    ("upscale.failed", "{count} failed", "{count} 個失敗"),
    ("upscale.skipped", "{count} skipped", "{count} 個跳過"),
    ("upscale.cancelled", "cancelled", "已取消"),
    ("upscale.summary", "{parts} in {elapsed}", "{parts}, 共 {elapsed}"),
    ("upscale.frames", "{count} frames", "{count} 幀"),
    ("upscale.plan", "Plan", "處理計畫"),
    ("upscale.col_input", "input", "輸入"),
    ("upscale.col_output", "output", "輸出"),
    ("upscale.skip_prefix", "skip: {reason}", "跳過: {reason}"),
    ("upscale.cancelling", "cancelling:", "取消中:"),
    ("upscale.cancel_hint", "finishing cleanup (Ctrl-C again to force quit)", "正在清理暫存檔 (再按一次 Ctrl-C 強制結束)"),
    ("upscale.update_hint", "IVSR {version} is available (current {current}); run `ivsr update install`", "IVSR {version} 已推出 (目前 {current}); 執行 `ivsr update install` 更新"),
    ("skip.unsupported", "unsupported format", "不支援的格式"),
    ("skip.output_exists", "output already exists", "輸出檔已存在"),
    ("skip.video_unavailable", "video support unavailable: {detail}", "無法處理影片: {detail}"),
    ("stage.preparing", "preparing", "準備中"),
    ("stage.decoding", "decoding", "抽取影格"),
    ("stage.upscaling", "upscaling", "高畫質化中"),
    ("stage.filtering", "filtering", "濾鏡處理中"),
    ("stage.encoding", "encoding", "編碼中"),
    ("stage.finalizing", "finalizing", "收尾中"),
    // ---- engines ------------------------------------------------------------
    ("engines.col_id", " id", " id"),
    ("engines.col_name", "name", "名稱"),
    ("engines.col_status", "status", "狀態"),
    ("engines.col_models", "models", "模型數"),
    ("engines.col_location", "location", "位置"),
    ("status.ready", "ready", "可用"),
    ("status.missing", "not installed", "未安裝"),
    ("status.broken", "broken", "異常"),
    ("engines.install_hint", "install with `ivsr engines install <id>`", "以 `ivsr engines install <id>` 安裝"),
    ("engines.status_line", "status: {status}", "狀態: {status}"),
    ("engines.binary", "binary: {path}", "執行檔: {path}"),
    ("engines.installed", "installed: {release} ({asset})", "已安裝: {release} ({asset})"),
    ("engines.models_heading", "Models", "模型"),
    ("engines.no_models", "none available until the engine is installed", "安裝引擎後才會有模型"),
    ("engines.params_heading", "Parameters (-p KEY=VALUE)", "參數 (-p KEY=VALUE)"),
    ("engines.col_model", " model", " 模型"),
    ("engines.col_scales", "scales", "倍率"),
    ("engines.col_tags", "tags", "標籤"),
    ("engines.col_description", "description", "說明"),
    ("engines.col_key", "key", "參數"),
    ("engines.col_type", "type", "型別"),
    ("engines.col_default", "default", "預設值"),
    ("engines.already", "{name} is already available ({status})", "{name} 已可使用 ({status})"),
    ("engines.force_hint", "use --force to reinstall", "加上 --force 可重新安裝"),
    ("engines.not_installable", "{name} cannot be installed automatically", "{name} 無法自動安裝"),
    ("engines.unknown", "unknown engine `{id}`", "未知的引擎 `{id}`"),
    ("progress.resolving", "resolving", "查詢中"),
    ("progress.downloading", "downloading", "下載中"),
    ("progress.extracting", "installing", "安裝中"),
    ("engines.installed_ok", "installed {name}", "已安裝 {name}"),
    ("verify.sha256", "sha256 verified", "已驗證 SHA-256"),
    ("verify.size_only", "size checked; the release publishes no checksum", "僅驗證檔案大小; 該 release 未提供 checksum"),
    // ---- filters ------------------------------------------------------------
    ("filters.col_id", "id", "id"),
    ("filters.col_name", "name", "名稱"),
    ("filters.col_stages", "stages", "階段"),
    ("filters.col_description", "description", "說明"),
    ("filters.col_reference", "reference", "參考圖"),
    ("filters.stage_pre", "pre-processing", "前處理"),
    ("filters.stage_post", "post-processing", "後製"),
    ("filters.on", "on", "開啟"),
    ("filters.off", "off", "關閉"),
    ("filters.builtin_order", "(built-in order)", "(內建順序)"),
    ("filters.hint", "turn on with --pre / --post when upscaling, or `ivsr config set filters.post.enabled true`", "高畫質化時加上 --pre / --post 開啟, 或執行 `ivsr config set filters.post.enabled true`"),
    ("filters.stages_line", "stages: {stages}", "可用階段: {stages}"),
    ("filters.params_heading", "Parameters (-F {id}.KEY=VALUE)", "參數 (-F {id}.KEY=VALUE)"),
    ("filters.unknown", "unknown filter `{id}` (see `ivsr filters`)", "未知的濾鏡 `{id}` (見 `ivsr filters`)"),
    ("filters.bad_param", "expected ID.KEY=VALUE, got `{pair}`", "格式應為 ID.KEY=VALUE, 收到 `{pair}`"),
    ("filters.not_in_chain", "filter `{id}` is not in an enabled chain; add --pre or --post", "濾鏡 `{id}` 不在已開啟的處理鏈中; 請加上 --pre 或 --post"),
    ("filters.not_in_steps", "filter `{id}` is not among the steps", "濾鏡 `{id}` 不在處理步驟中"),
    ("filters.reference", "reference: {path}", "參考圖: {path}"),
    ("filters.no_reference", "no reference", "無參考圖"),
    ("filters.video_skipped", "videos are filtered while upscaling (--pre / --post)", "影片請在高畫質化時處理 (--pre / --post)"),
    // ---- models -------------------------------------------------------------
    ("models.col_status", "status", "狀態"),
    ("models.col_class", "cost", "負載"),
    ("models.col_size", "size", "大小"),
    ("models.col_license", "license", "授權"),
    ("models.status.bundled", "bundled", "內建"),
    ("models.status.installed", "installed", "已安裝"),
    ("models.status.update_available", "update", "可更新"),
    ("models.status.imported", "imported", "已匯入"),
    ("models.status.available", "available", "可下載"),
    ("class.light", "light", "輕量"),
    ("class.medium", "medium", "中等"),
    ("class.heavy", "heavy", "重量"),
    ("models.default_marker", "* = default model", "* = 預設模型"),
    ("models.hint_install", "download with `ivsr models install <id>`; details with `ivsr models show <id>`", "以 `ivsr models install <id>` 下載; 以 `ivsr models show <id>` 查看詳情"),
    ("models.hint_updates", "{count} update(s) available: `ivsr models update --all`", "有 {count} 個模型可更新: `ivsr models update --all`"),
    ("models.catalog_error", "catalogue {url} unavailable: {reason}", "無法載入型錄 {url}: {reason}"),
    ("models.unknown", "unknown model `{id}` (see `ivsr models`)", "未知的模型 `{id}` (見 `ivsr models`)"),
    ("models.not_installed", "model `{id}` is not installed", "模型 `{id}` 尚未安裝"),
    ("models.license", "license: {license}", "授權: {license}"),
    ("models.author", "author: {author}", "作者: {author}"),
    ("models.version", "version: {version}", "版本: {version}"),
    ("models.source", "catalogue: {source}", "型錄來源: {source}"),
    ("models.scales", "scales: {scales}", "倍率: {scales}"),
    ("models.size_line", "size: {size}", "大小: {size}"),
    ("models.download_size", "download: {size}", "下載大小: {size}"),
    ("models.parameters", "weights: {count}", "權重數: {count}"),
    ("models.attribution", "Attribution required by the licence: credit {author} and link {homepage}.", "此授權要求標示出處: 註明 {author} 並附上 {homepage}."),
    ("models.hardware_heading", "Hardware", "硬體需求"),
    ("models.class_line", "cost class: {class}", "負載等級: {class}"),
    ("models.col_tile", "tile", "tile"),
    ("models.col_memory", "peak memory", "峰值記憶體"),
    ("models.tile_auto", "auto", "自動"),
    ("models.memory_note", "Measured on {device}; other GPUs differ. Smaller tiles need less memory but run slower.", "於 {device} 量測; 其他 GPU 會有差異. tile 越小越省記憶體, 但速度較慢."),
    ("models.baseline_heading", "Baseline", "Baseline 速度"),
    ("models.reference", "reference: {device}", "參考裝置: {device}"),
    ("models.local", "this machine: {device} (measured {date})", "這台電腦: {device} (量測於 {date})"),
    ("models.col_input", "input", "輸入"),
    ("models.col_time", "time", "時間"),
    ("models.per_mp", "≈ {startup}s start-up + {rate}s per input megapixel", "≈ 啟動 {startup} 秒 + 每百萬輸入像素 {rate} 秒"),
    ("models.est_frame", "≈ {time} per 1920×1080 input frame", "1920×1080 輸入每幀約 {time}"),
    ("models.no_local", "No benchmark on this machine yet: `ivsr models bench {id}`", "這台電腦尚未測速: `ivsr models bench {id}`"),
    ("models.advice_heading", "On this machine", "這台電腦的建議"),
    ("advice.comfortable", "{gpu}: enough memory ({available} available, {needed} needed); automatic tiling is fine.", "{gpu}: 記憶體充足 (可用 {available}, 需要 {needed}); 使用自動 tile 即可."),
    ("advice.constrained", "{gpu}: {available} available, {needed} needed with automatic tiling; use `-p tile={tile}`.", "{gpu}: 可用 {available}, 自動 tile 需要 {needed}; 建議使用 `-p tile={tile}`."),
    ("advice.insufficient", "{gpu}: only {available} available; even `-p tile={tile}` may fail. Prefer a light model.", "{gpu}: 只有 {available} 可用; 即使 `-p tile={tile}` 也可能失敗, 建議改用輕量模型."),
    ("advice.unknown", "GPU memory unknown; this model needs about {needed} with automatic tiling.", "無法得知 GPU 記憶體; 此模型以自動 tile 約需 {needed}."),
    ("advice.no_gpu", "No GPU reported by the engine.", "引擎未回報任何 GPU."),
    ("models.installing", "installing {id}", "安裝 {id}"),
    ("models.installed_ok", "installed {id} ({size})", "已安裝 {id} ({size})"),
    ("models.up_to_date", "{id} is up to date", "{id} 已是最新"),
    ("models.no_updates", "all models are up to date", "所有模型都是最新版"),
    ("models.updated_ok", "updated {id}", "已更新 {id}"),
    ("models.confirm_remove", "Remove {id} ({size})?", "要移除 {id} ({size}) 嗎?"),
    ("models.removed", "removed {id}", "已移除 {id}"),
    ("models.default_cleared", "{id} was the default model; the engine default is used now", "{id} 原本是預設模型; 現在改用引擎預設"),
    ("models.imported", "imported {id}: verified x{scale} on a test image", "已匯入 {id}: 已用測試圖驗證 x{scale}"),
    ("models.use_ok", "default model: {id}", "預設模型: {id}"),
    ("models.bench_running", "benchmarking {id} x{scale}", "測速 {id} x{scale}"),
    ("models.bench_result", "{id} x{scale} on {device}: {t256} (256²), {t512} (512²) → {frame} per 1080p frame", "{id} x{scale} 於 {device}: {t256} (256²)、{t512} (512²) → 1080p 每幀約 {frame}"),
    ("models.bench_none", "no models to benchmark", "沒有可測速的模型"),
    ("models.yes_hint", "pass --yes to remove without a prompt", "加上 --yes 可不經確認直接移除"),
    // ---- system -------------------------------------------------------------
    ("system.heading", "System", "系統"),
    ("system.os", "OS: {os} ({arch})", "作業系統: {os} ({arch})"),
    ("system.cpu", "CPU: {cpu} · {threads} threads", "CPU: {cpu} · {threads} 執行緒"),
    ("system.memory", "Memory: {memory}", "記憶體: {memory}"),
    ("system.gpus", "GPUs ({engine})", "GPU ({engine})"),
    ("system.gpu_unified", "shares system memory", "與系統共用記憶體"),
    ("system.gpu_memory", "{memory}", "{memory}"),
    ("system.gpu_unknown", "memory unknown", "記憶體未知"),
    ("system.no_gpus", "none detected (is the engine installed?)", "未偵測到 (引擎是否已安裝?)"),
    ("system.tools", "Tools", "工具"),
    ("system.unknown", "unknown", "未知"),
    // ---- formats / probe ----------------------------------------------------
    ("formats.images", "Images", "圖片"),
    ("formats.containers", "Video containers", "影片容器"),
    ("formats.codecs", "Video codecs", "影片 codec"),
    ("formats.col_id", "id", "id"),
    ("formats.col_extensions", "extensions", "副檔名"),
    ("formats.col_read", "read", "讀取"),
    ("formats.col_write", "write", "寫入"),
    ("formats.col_note", "note", "備註"),
    ("formats.col_codec", "codec", "codec"),
    ("formats.col_containers", "containers", "容器"),
    ("formats.col_quality", "quality", "品質"),
    ("formats.col_available", "available", "可用"),
    ("formats.video_unavailable", "video support unavailable: {problem}", "無法處理影片: {problem}"),
    ("probe.image", "image  {w}×{h}  {format}  alpha: {alpha}", "圖片  {w}×{h}  {format}  透明度: {alpha}"),
    ("probe.video", "video  {w}×{h}  {fps} fps  {codec}  {frames}  {duration}s  audio: {audio}", "影片  {w}×{h}  {fps} fps  {codec}  {frames}  {duration} 秒  音軌: {audio}"),
    ("probe.frames", "{count} frames", "{count} 幀"),
    ("probe.no_audio", "none", "無"),
    // ---- update -------------------------------------------------------------
    ("update.not_configured", "no update source is configured", "尚未設定更新來源"),
    ("update.configure_hint", "ivsr config set update.repository <owner>/<repo>", "ivsr config set update.repository <owner>/<repo>"),
    ("update.up_to_date", "IVSR {version} is up to date", "IVSR {version} 已是最新版本"),
    ("update.available", "IVSR {latest} is available (current {current})", "IVSR {latest} 已推出 (目前 {current})"),
    ("update.install_hint", "run `ivsr update install`", "執行 `ivsr update install` 安裝"),
    ("update.skipped", "will not remind about {version} again", "不再提醒 {version}"),
    ("update.no_asset", "release {tag} has no CLI build for this platform", "版本 {tag} 沒有此平台的 CLI 建置"),
    ("update.confirm", "Install IVSR {version} ({size})?", "要安裝 IVSR {version} ({size}) 嗎?"),
    ("update.yes_hint", "pass --yes to install without a prompt", "加上 --yes 可不經確認直接安裝"),
    ("update.updated", "updated to IVSR {version}: {path}", "已更新至 IVSR {version}: {path}"),
    // ---- config -------------------------------------------------------------
    ("config.not_set", "`{key}` is not set", "`{key}` 未設定"),
    ("config.reset", "{key} reset to default", "{key} 已恢復預設"),
    ("config.config", "config", "設定檔"),
    ("config.data", "data", "資料"),
    ("config.cache", "cache", "快取"),
];

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use super::*;

    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let Some(len) = rest[start..].find('}') else { break };
            let name = &rest[start + 1..start + len];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.insert(name.to_string());
            }
            rest = &rest[start + len + 1..];
        }
        out
    }

    #[test]
    fn keys_are_unique_and_translations_keep_placeholders() {
        let mut seen = HashSet::new();
        for (key, en, zh) in MESSAGES {
            assert!(seen.insert(*key), "duplicate key {key}");
            assert!(!zh.is_empty() || key.starts_with("arg.") && en.is_empty(), "{key} lacks zh-TW");
            // Help rows have an empty English column: clap's derive text is the source.
            if !en.is_empty() && !key.starts_with("arg.") && *key != "after_help" && *key != "usage" {
                assert_eq!(placeholders(en), placeholders(zh), "placeholders differ for {key}");
            }
        }
    }

    #[test]
    fn every_key_used_in_the_sources_exists() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let src = std::fs::read_to_string(&path).unwrap();
                for (i, _) in src.match_indices("tr!(\"") {
                    let key: String = src[i + 5..].chars().take_while(|c| *c != '"').collect();
                    if lookup(Lang::ZhTw, &key).is_none() {
                        missing.push(format!("{}: {key}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "missing keys:\n{}", missing.join("\n"));
    }

    #[test]
    fn localized_help_builds_for_every_subcommand() {
        init(Lang::ZhTw);
        let mut cmd = localize(<crate::cli::Cli as clap::CommandFactory>::command());
        cmd.build();
        let help = cmd.render_help().to_string();
        assert!(help.contains("圖片與影片超解析度工具"), "{help}");
        let models = cmd.find_subcommand_mut("models").unwrap();
        assert!(models.render_help().to_string().contains("安裝、更新、移除、匯入與測速模型"));
    }
}
