# IVSR 架構與可行性評估

## 結論

可行, 第一版已完成並在 macOS (Apple M4 Pro) 上端到端驗證: CLI 與 GUI 都能用 Real-ESRGAN 處理圖片與影片, 引擎可由 GitHub Releases 自動下載安裝, 更新檢查可接真實的 GitHub API. 第二版加入英文 / 正體中文介面、模型管理 (安裝、更新、移除、匯入、測速) 與硬體建議, 以及桌面版的成果瀏覽與比較檢視器.

主要風險與對策:

| 風險 | 狀態 / 對策 |
| --- | --- |
| Real-ESRGAN ncnn 版最後一次發布是 2022 年 (v0.2.5.0) | 已驗證 macOS 版是 universal binary, 在 M4 Pro 以 Vulkan (MoltenVK) 正常執行. 引擎走介面, 日後可替換為其他實作 |
| 影片逐幀處理的暫存空間 (1080p→4K 的 PNG 每幀約 20 MB) | 來源幀先以原解析度抽出, 再分批超分, 每批結果立即串流進常駐的 ffmpeg 編碼器並刪除. 任何時刻只有一批大圖在磁碟上 |
| `realesrgan-ncnn-vulkan` 找不到模型檔時直接 crash (exit 139, 無訊息) | 實驗確認它會載入任意 `<name>.param/.bin` (只有 `realesr-animevideov3` 這個名稱改讀 `-x<scale>` 檔), 所以執行前一律先確認權重檔存在 |
| 模型目錄路徑必須包含 `models` 字樣 (binary 限制) | `status()` 會檢查並回報 Broken, 不會等到執行時才失敗 |
| 舊版 release 資產沒有 checksum | GitHub 有提供 `digest` 時驗證 SHA-256, 否則只驗證檔案大小, 並在 CLI/安裝紀錄中如實標示 `size_only` |
| GUI 自我更新 | 下載平台安裝檔 (`.dmg` / `-setup.exe` / `.msi` / `.AppImage`) 後交給系統開啟; CLI 則就地替換執行檔 |
| `image` crate 的 WebP 編碼只有無損 | 格式表明確標示, 有損 WebP 留待之後加 codec 實作 |
| 下載的模型可能宣告錯誤的倍率或損壞 | 型錄檔案逐一驗證 SHA-256; 匯入的模型會先用 16×16 測試圖實際跑一次, 輸出倍率不符就整個撤回 |
| 第三方模型授權 | 型錄只收錄授權明確的模型 (BSD-3-Clause、CC-BY-4.0, 以 OpenModelDB 為準), 並在 CLI / GUI 顯示作者與出處; CC-BY-NC 等非商用授權未收錄 |

## 分層與相依方向

```text
            ┌──────────────┐        ┌───────────────────────────┐
            │   ivsr-cli   │        │ apps/desktop (Tauri+Svelte)│
            │  (clap, UI)  │        │  src-tauri: IPC adapter    │
            └──────┬───────┘        └─────────────┬─────────────┘
                   │      thin adapters only      │
                   └──────────────┬───────────────┘
                                  ▼
                       ┌──────────────────────┐
                       │     ivsr-service     │  composition root:
                       │ config · registry ·  │  config.toml, planner,
                       │ planner · queue ·    │  job queue, engine install,
                       │ engines · updates    │  self-update policy
                       └──┬─────┬─────┬────┬──┘
          ┌───────────────┘     │     │    └──────────────────┐
          ▼                     ▼     ▼                       ▼
┌───────────────────────┐ ┌──────────────┐ ┌─────────────┐ ┌────────────────┐
│ ivsr-engine-realesrgan│ │ ivsr-filters │ │ ivsr-media  │ │  ivsr-update   │
│   impl Engine         │ │ impl Filter  │ │ impl ImageIo│ │ (standalone;   │
└──────────┬────────────┘ └──────┬───────┘ │ impl VideoIo│ │  no ivsr deps) │
           │                     │         └──────┬──────┘ └────────────────┘
           ▼                     ▼                ▼
        ┌──────────────────────────────────────────────┐
        │                  ivsr-core                   │  traits + pipeline,
        │ Engine · Filter · ImageIo · VideoIo ·        │  no tools, no config,
        │ Reporter · ParamSpec · plan                  │  no frontends
        └──────────────────────────────────────────────┘
```

相依只往下走. `ivsr-core` 不認識任何具體工具; `ivsr-update` 完全獨立, 可以直接拿去給別的產品用.

## 權威介面

全部定義在 `crates/ivsr-core/src`:

| 介面 | 職責 | 目前實作 |
| --- | --- | --- |
| `Engine` (`engine.rs`) | 自我描述 (`info`, `models`, `params`, `caps`, `distribution`) 並執行 `upscale(UpscaleTask)` | `RealEsrgan` |
| `Filter` / `FilterRun` (`filter.rs`) | 前處理 / 後製濾鏡: 自我描述 (`info` 含可用階段, `params`), 每個 job `start` 一個有狀態的 run, 依顯示順序逐張 `apply(Frame, reference)` | `alpha-bleed`、`tone-restore`、`detail-sharpen`、`saturation` |
| `ImageIo` (`media.rs`) | 圖片 `probe` 與 `convert` (解碼、重取樣、編碼), 以及濾鏡用的 `decode` / `encode` (RGBA8 `Frame`) | `RasterIo` (image + fast_image_resize) |
| `VideoIo` + `FrameEncoder` (`media.rs`) | 影片 `probe`、抽幀、開啟串流編碼器 | `Ffmpeg` |
| `Reporter` / `TaskContext` (`progress.rs`) | 進度與日誌回報, 取消權杖 | CLI 進度條、JSON 事件、GUI Channel |
| `ParamSpec` / `ParamValues` (`params.rs`) | 引擎參數 schema 與驗證 | CLI `-p key=value`、GUI 動態表單共用 |
| `ReleaseSource` / `AssetSelector` / `HttpClient` (`ivsr-update`) | 發布來源、平台資產挑選、HTTP 傳輸 | `GitHubSource`、`PlatformSelector`、`UreqClient` |
| `Engine::catalog` / `model_store` / `model_layout` / `devices` | 引擎提供內建型錄、受管模型目錄、模型檔案配置規則、可用的運算裝置 | Real-ESRGAN: `catalog.json`, `models/<id>.{param,bin}` |
| `Text` (`i18n.rs`) | 引擎與型錄提供的多語文字, 依語言回退到英文 | 所有模型說明、參數標籤、格式備註 |

`pipeline::run` 只依賴這些 trait: 檢查引擎 → 規劃縮放倍率 → 在私有暫存目錄處理 → 成功時以 rename 原子化產出; 任何結束路徑 (失敗、取消) 都會刪除暫存目錄, 取消時會 kill 子程序.

## 處理流程

圖片:

```text
probe ─▶ (有前處理: decode ─▶ pre 濾鏡 ─▶ PNG | 引擎不吃該格式時先轉 PNG) ─▶ Engine::upscale(native scale)
      ─▶ 有後製: decode(縮放到最終尺寸) ─▶ post 濾鏡 (參考圖 = 送進引擎的那張) ─▶ encode
         沒有後製: (倍率非原生 或 格式不同時) ImageIo::convert(resize + encode)
      ─▶ persist
```

倍率規劃 (`scale.rs`): 有原生倍率就直接用; 否則用大於需求的最小原生倍率, 再以 Lanczos3 縮回. 例如 x4plus 只有 x4, 使用者要 x2 時模型跑 x4 再縮到 x2.

影片:

```text
probe ─▶ extract_frames (CFR, 原解析度) ─▶ 每批 N 幀: pre 濾鏡 (就地改寫來源幀) ─▶ Engine::upscale(dir)
                                                  └─▶ post 濾鏡 (縮放到最終尺寸, 參考圖 = 同一幀來源)
                                                  └─▶ FrameEncoder::push_frame (ffmpeg stdin) ─▶ 刪除結果與來源幀
      ─▶ FrameEncoder::finish (帶原音軌) ─▶ persist
```

抽幀固定用來源的平均幀率 (CFR), 讓幀數與音軌長度一致. 有後製時幀已是最終尺寸, 編碼器不再縮放. 批次大小 `video.batch_frames` (預設 48) 控制暫存空間與引擎啟動次數之間的取捨.

## 擴充指南

新增引擎:

1. 新增 crate 實作 `ivsr_core::Engine`. 參數用 `ParamSpec` 描述, CLI 與 GUI 不用改.
2. 若可自動安裝, 實作 `distribution()` 與 `install_dir()`.
3. 在 `crates/ivsr-service/src/registry.rs` 的 `builtin_engines` 加一行.

新增濾鏡:

1. 在 `crates/ivsr-filters` 實作 `ivsr_core::Filter`: `info` 宣告 id、多語名稱與可用階段 (`pre` / `post`), 參數用 `ParamSpec` 描述; `start` 回傳的 `FilterRun` 可保留跨幀狀態 (影片逐幀依序送入). 完全透明的像素不得改動, 除非那正是濾鏡的用途.
2. 在 `ivsr_filters::builtin` 加一行; 要放進內建順序就改 `ivsr_filters::default_steps`. CLI (`ivsr filters`、`-F id.key=value`) 與 GUI (設定面板、檢視器濾鏡面板) 都直接讀 schema, 不用改.

新增格式 / codec: 圖片格式加在 `crates/ivsr-media/src/image_io.rs` 的 `FORMATS`, 影片 codec 加在 `crates/ivsr-media/src/ffmpeg/codecs.rs` 的 `CODECS`. 前端選單直接讀這兩張表.

新增更新來源: 實作 `ivsr_update::ReleaseSource`, 並在 `crates/ivsr-service/src/engines.rs` 的 `source_for` 對應 `update.provider` 字串.

## 模型管理

```text
內建 catalog.json ─┐
遠端型錄 (https) ──┼─▶ load_catalogs (先到先得, 遠端不能覆蓋內建 id; 每日快取, 離線時用舊快取)
                   │
Engine::models() ──┴─▶ overview: bundled / installed / update_available / imported / available
                                       │
install ─▶ 逐檔下載 + SHA-256 ─▶ .staging-<id>/ ─▶ rename 成 <store>/<id>/   (失敗或取消不留殘檔)
import  ─▶ 檢查 ncnn magic ─▶ 複製 ─▶ 實際跑 16×16 測試圖驗證倍率 ─▶ 不符則撤回
remove  ─▶ 只允許受管模型; 若是預設模型, 同時清除設定中的預設值
```

受管模型放在 `<data>/models/<engine>/<id>/`, 內含 `model.json` (manifest) 與引擎決定的檔案配置. 是否「可更新」以檔案雜湊判斷: 型錄上同一 id 的 SHA-256 與已安裝的不同即為可更新, 不依賴版本字串.

## 硬體建議與 baseline

- 每個架構 (`rrdb`、`rrdb-6b`、`compact`) 在型錄中有一份硬體資料: 負載等級, 以及在參考裝置上用 `/usr/bin/time -l` 量到的各 tile 大小峰值記憶體.
- 每個模型有兩個 baseline 點 (256² 與 512² 輸入的單次執行時間). 以最小平方法擬合成 `秒數 ≈ 啟動時間 + 每百萬輸入像素秒數 × 百萬像素`, 用來估算 1080p 每幀時間與佇列的預估時間.
- `ivsr models bench` (或 GUI 的「測速」) 在使用者自己的 GPU 上重跑同樣的量測, 結果存在 `<data>/benchmarks.json`. 有本機結果時一律優先使用; 沒有時只在 GPU 與參考裝置相同時才拿參考值做預估, 否則只顯示參考數據, 不冒充成本機估計.
- 建議邏輯 (`advice.rs`): 獨立顯卡可用記憶體取 85%, Apple Silicon 統一記憶體取 60%; 自動 tile 放得下就「順暢」, 否則建議放得下的最大 tile, 連最小 tile 都放不下就建議改用輕量模型. 獨立顯卡的記憶體目前只能從 `nvidia-smi` 取得, 其他廠牌顯示為未知.

參考量測 (Apple M4 Pro, 24 GB 統一記憶體):

| 架構 | 模型 | 256² | 512² | 自動 tile 峰值記憶體 |
| --- | --- | --- | --- | --- |
| rrdb | realesrgan-x4plus | 1.22 s | 3.35 s | 1480 MB |
| rrdb-6b | realesrgan-x4plus-anime | 0.61 s | 1.26 s | 1215 MB |
| compact | realesr-animevideov3 (x2) | 0.17 s | 0.32 s | 190 MB |
| compact | realesr-general-x4v3 | 0.31 s | 0.73 s | 250 MB |

## 多語系

- 語言決定順序: `--lang` / `IVSR_LANG` → 設定檔 `ui.language` → 作業系統語言 → 英文. 繁體中文地區 (`zh-TW`、`zh-HK`、`zh-Hant`) 對應正體中文; 簡體中文目前沒有翻譯, 會使用英文.
- CLI: 訊息集中在 `crates/ivsr-cli/src/i18n.rs` 的 `(key, en, zh-TW)` 表, 以 `tr!` 取用; 說明文字在解析參數前依語言改寫 clap 指令樹. 測試會檢查每個用到的 key 都存在、譯文保留相同的 `{placeholder}`.
- 桌面版: `apps/desktop/src/lib/i18n/` 的英文字典是權威來源, 正體中文字典的型別強制 key 完全一致, `pnpm check` 另外檢查 placeholder. 語言是一個 `$state`, 切換時只有用到翻譯的節點重繪.
- 引擎、型錄與格式表的文字用 `Text` 型別承載多語內容, 前端只負責挑選語言.
- `--json` 輸出不翻譯 (key 與狀態碼維持英文), 方便腳本使用.

## 成果瀏覽與比較檢視器

- `Service::run` 每完成一個 job 就寫入 `<data>/history.json` (保留最新 500 筆, 同一輸出檔只留最新一筆). CLI 與 GUI 共用, 所以命令列跑的結果也會出現在 GUI 的瀏覽頁.
- 檢視器的兩個圖層共用同一個 transform, 縮放與平移在三種模式中永遠對齊; 原圖以結果的尺寸繪製, 放大到超過 1:1 時切換成 nearest-neighbor, 呈現真實像素. 切換模式時保持畫面中心不變. 影片模式以結果影片為主時鐘, 每幀校正原片的播放位置.
- 濾鏡面板 (`F`) 在核心端算出預覽檔: 對超分後的圖套後製 (以超分前為參考), 或對超分前的圖套前處理. 「套用濾鏡」總開關 (`\`) 關閉時不計算預覽; 開啟時套用濾鏡的圖一律在右側圖層, 左側依「對照」選擇未套用的同一張圖或另一側. 畫面上的圖層標籤、面板的「左 / 右」說明與左上角狀態標籤都由同一份圖層資料產生, 三處不會不一致. 影片只能在放大時處理.
- 介面大小用 webview 的頁面縮放 (`setZoom`), 會讓 CSS viewport 變窄, 所以 RWD 只需一個 760px 斷點: 標題列改為圖示、設定面板改為抽屜、檢視器標題列換行、濾鏡面板浮在圖片上.

## GUI 設計重點

- Rust 端是唯一做事的地方: 媒體探測、縮圖 (`Service::thumbnail`, 160px JPEG 快取)、檔案存取、子程序管理都在核心服務. Webview 只負責呈現與送出意圖.
- 進度用 `tauri::ipc::Channel` 串流, 核心端節流到每個 job 約 10 Hz.
- 每個佇列項目是獨立的 Svelte 5 `$state` 物件, 一次進度更新只重繪該列; 進度條用 `transform: scaleX` 更新, 不觸發 layout.
- 拖放使用 Tauri 原生 drag-drop 事件 (拿得到檔案路徑). asset protocol 的 scope 一開始是空的, 只對縮圖快取目錄、濾鏡預覽快取目錄 (每次啟動清空)、加入佇列的圖片與產出檔逐一放行.
- GUI 與 CLI 共用同一份 `config.toml`, 在 GUI 改的預設值 CLI 立即沿用.

## 設定檔

位置: `ivsr config path` (可用 `IVSR_HOME` 把設定、資料、快取全部移到同一目錄, 適合可攜版與測試).

```toml
engine = "realesrgan"

[output]
scale = 4.0
image_format = "same"     # 或 png / jpg / webp / ...
image_quality = 92
suffix = "_x{scale}"      # 可用 {scale} {model} {engine}
conflict = "rename"       # rename / overwrite / skip

[video]
codec = "h264"            # h264 / h265 / av1 / vp9 / prores
audio = "auto"            # auto / copy / reencode / drop
container = "same"
batch_frames = 48

[engines.realesrgan]
model = "realesrgan-x4plus"
[engines.realesrgan.params]
tile = 0

[filters.pre]               # 前處理: 放大前處理來源
enabled = false
[filters.post]              # 後製: 以最終尺寸處理成果
enabled = false             # 沒有 steps 時沿用內建順序
# [[filters.post.steps]]    # 自訂順序: 依序列出, 可停用個別步驟或覆寫參數
# id = "detail-sharpen"
# params = { amount = 1.4 }

[update]
provider = "github"
repository = "Yuhuan183/IVSR"  # owner/repo; 空字串代表停用更新檢查
channel = "stable"        # stable / beta
auto_check = true
interval_hours = 24
token_env = ""            # 私有 repo: 存放 token 的環境變數名稱
```

## 產品自身的發布慣例

更新框架依資產檔名挑選平台:

- CLI: `ivsr-cli-<version>-<target-triple>.tar.gz` (Windows 用 `.zip`), 壓縮檔內含 `ivsr` / `ivsr.exe`.
- 桌面版: Tauri bundler 預設產出的 `.dmg`、`-setup.exe`、`.msi`、`.AppImage`.

發布 repo 預設為 `Yuhuan183/IVSR`; 可在建置時以 `IVSR_UPDATE_REPOSITORY=owner/repo` 改寫預設值, 或由使用者以 `ivsr config set update.repository owner/repo` 設定 (設為空字串即停用更新檢查).

## 目前的限制

- 只實作了 GitHub 一種更新來源.
- 瀏覽頁的影片目前只顯示圖示, 沒有影格縮圖.
- 核心與服務層回傳的錯誤訊息 (例如找不到模型、參數超出範圍) 目前只有英文; 介面文字與 CLI 訊息已翻譯.
- 非 NVIDIA 的獨立顯卡無法取得記憶體容量, 硬體建議只會列出需求.
- 桌面版 macOS 安裝檔未簽章 / 未公證; 正式發布前需要 Developer ID 與 notarization.
- 影片音軌以外的串流 (字幕、章節) 不會帶到輸出.
- 可變幀率 (VFR) 影片會被轉成固定幀率.
