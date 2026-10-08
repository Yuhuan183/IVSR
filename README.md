# IVSR

輕量的圖片 / 影片超解析度工具: 以命令列為核心, 另有 Tauri 桌面版. 目前的引擎是 Real-ESRGAN (ncnn + Vulkan), 影片透過 ffmpeg 處理. 介面支援英文與正體中文.

架構、可行性評估與擴充方式見 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## 需求

- Rust 1.88+
- ffmpeg / ffprobe 在 `PATH` 上 (只有處理影片時需要)
- 桌面版另需 Rust 1.90+、Node 22+ 與 pnpm

## 建置

```sh
cargo build --release -p ivsr-cli        # CLI: target/release/ivsr
cargo test                                # 核心、引擎、媒體、更新、服務、CLI 測試

cd apps/desktop
pnpm install
pnpm check                                # 型別檢查與翻譯一致性檢查
pnpm tauri dev                            # 開發模式
pnpm tauri build                          # 打包 .app / .dmg / .msi / ...
```

## CLI

```sh
ivsr engines install realesrgan          # 首次使用: 下載 Real-ESRGAN 執行檔與內建模型
ivsr photo.jpg                           # x4, 輸出 photo_x4.jpg
ivsr -s 2 -f webp shots/ -o out/ -r      # 整個資料夾 (含子目錄), x2, 轉 WebP
ivsr -m realesrgan-x4plus-anime art.png  # 指定模型
ivsr -p tile=256 -p tta=true big.png     # 引擎參數 (見 ivsr engines show realesrgan)
ivsr clip.mp4 -m realesr-animevideov3 --codec h265 --crf 20
ivsr -n shots/                           # dry run: 只顯示會產生哪些檔案
ivsr --json clip.mp4                     # 每行一個 JSON 事件, 方便腳本整合
ivsr --post icon.png                     # 高畫質化後以內建順序後製 (階調還原、細節銳化...)
ivsr --pre --post=tone-restore clip.mp4  # 影片也適用; --post=a,b 依序指定步驟
ivsr --post -F detail-sharpen.amount=1.4 gems/   # 調整單一濾鏡參數
ivsr --lang zh-TW engines                # 介面語言 (也可用 IVSR_LANG 或 ui.language)
```

`ivsr <檔案>` 等同 `ivsr upscale <檔案>`; 完整選項見 `ivsr upscale --help`. 處理中按 Ctrl-C 會取消並清除暫存檔, 再按一次則立即結束.

### 前處理與後製

濾鏡分兩段: 前處理在高畫質化前處理原圖, 後製以最終尺寸處理高畫質結果, 並以送進引擎的那張圖為參考. 每段有總開關和有序的步驟, 預設都關閉. 內建濾鏡與原理見 [docs/SPRITE_POST_PROCESSING.md](docs/SPRITE_POST_PROCESSING.md).

```sh
ivsr filters                             # 濾鏡、可用階段與目前的處理鏈
ivsr filters show detail-sharpen         # 參數說明
ivsr filters apply out/icon_x4.png       # 只套用濾鏡 (圖片); 原圖自動從歷史紀錄找回, 或用 --reference 指定
ivsr config set filters.post.enabled true   # 預設開啟後製
```

### 模型管理

```sh
ivsr models                              # 已安裝與可下載的模型、負載等級、授權
ivsr models show 4x-LSDIRCompactC3       # 詳情: 硬體需求 (各 tile 的峰值記憶體)、baseline 速度、這台電腦的建議
ivsr models install 4x-LSDIRCompactC3    # 從型錄下載 (逐檔驗證 SHA-256)
ivsr models update --all                 # 型錄檔案變更時更新
ivsr models use 4x-LSDIRCompactC3        # 設為預設模型
ivsr models bench --all                  # 在這台電腦測速, 結果用於時間預估
ivsr models import my-net --param my.param --bin my.bin --scale 4   # 匯入本機 ncnn 模型 (會實際驗證倍率)
ivsr models remove my-net                # 移除下載或匯入的模型 (內建模型不可單獨移除)
ivsr system                              # OS、CPU、記憶體、引擎可用的 GPU
```

額外的模型型錄可用 `ivsr config set models.catalogs '["https://example.com/catalog.json"]'` 加入, 格式同 [`crates/ivsr-engine-realesrgan/src/catalog.json`](crates/ivsr-engine-realesrgan/src/catalog.json).

## 桌面版

- **高畫質化**: 拖放或選擇檔案 / 資料夾, 依模型的本機測速顯示每個項目的預估時間.
- **前處理 / 後製**: 設定面板可開關兩段處理, 並調整步驟的順序、開關與參數; 設定與 CLI 共用.
- **瀏覽**: 列出 app 與 CLI 完成的成果. 比較檢視器有三種模式: 滑桿 (可拖曳分隔線)、並排雙窗格、淡入淡出; 兩側同步縮放平移, 影片同步播放. 快捷鍵: `1`/`2`/`3` 切換模式、滾輪縮放、`0` 符合視窗、`←`/`→` 上下一個、空白鍵播放、`F` 濾鏡面板、`\` 開關濾鏡. 濾鏡面板可對原圖或高畫質結果試套濾鏡, 並選擇對照未套用的同一張圖或另一側; 畫面左上角隨時標示濾鏡是否已套用. 可另存套用後的圖, 或把步驟設為工作流預設.
- **模型**: 硬體資訊、模型安裝 / 更新 / 測速 / 移除 / 匯入, 以及硬體需求與 GPU 建議.
- 右上角可切換語言 (跟隨系統 / English / 正體中文). 文字大小圖示可調整介面大小 (100%–200%, 也可用 Cmd/Ctrl 加 `+` `-` `0`), 比較檢視器的工具列也有同一個按鈕; 視窗變窄或介面放大時版面會自動調整, 設定面板改為側拉抽屜. 設定與 CLI 共用.
- 設定面板與濾鏡面板可拖曳左緣調整寬度 (按兩下恢復預設).
- 倍率可選 ×1: 模型以原生倍率處理後縮回原尺寸, 尺寸不變但細節更清楚.

## 專案結構

```text
crates/
  ivsr-core/               介面 (Engine, Filter, ImageIo, VideoIo, Reporter, ParamSpec)、模型 / 型錄型別、處理流程
  ivsr-engine-realesrgan/  Real-ESRGAN 引擎實作與內建模型型錄 (catalog.json)
  ivsr-filters/            內建前處理 / 後製濾鏡 (prototype/ 為最初的 Python 原型)
  ivsr-media/              圖片 (image-rs) 與影片 (ffmpeg) I/O 實作
  ivsr-update/             獨立的更新框架 (ReleaseSource, GitHub, 下載驗證, 解壓, 自我替換)
  ivsr-service/            組合根: 設定、語言、registry、規劃、佇列、模型管理、測速、歷史紀錄、更新策略
  ivsr-cli/                ivsr 命令列 (含訊息翻譯表)
apps/desktop/              Tauri 2 + Svelte 5 桌面版 (src/lib/i18n 為翻譯字典)
docs/
  ARCHITECTURE.md          架構文件
  SPRITE_POST_PROCESSING.md 遊戲 Sprite / 圖標超分後處理指南
```

## 授權

本專案尚未選定授權. 執行時下載的 Real-ESRGAN 執行檔與模型各自依其授權 (BSD-3-Clause、CC-BY-4.0 等) 發布; CC-BY 模型的出處標示會顯示在 `ivsr models show` 與桌面版模型頁.
