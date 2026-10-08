# Sprite / 遊戲圖標超解析度後處理指南

本文件說明當使用 Real-ESRGAN 等 AI 超解析度模型放大 2D/3D 遊戲圖標、去背 Sprite 時，可能發生的**對比度降低、高光與暗部發灰、微細節銳利感平淡**等問題之成因，以及隨附的專門後處理管線與使用方法。

---

## 1. 問題成因分析

以 Real-ESRGAN 系列（如 `realesrgan-x4plus`、`4x-Nomos8kSC` 等）放大具有透明通道（RGBA）的 3D 渲染圖標時，常觀察到以下現象：

| 現象 | 成因 | 影響範例 |
| --- | --- | --- |
| **動態範圍壓縮 (Contrast Compression)** | 模型為抑制偽影，訓練目標與卷積層存在隱式平滑與降噪傾向，導致高光被稍微拉低、深黑暗部被拉高。實測亮度標準差（Contrast 指標）下降約 6%~10% | 跑車輪框發灰失去深黑、花瓣陰影褶皺變淺、寶石折射暗部變薄 |
| **微對比度 (Micro-contrast) 平淡** | 內部高頻邊緣與細微刻線被過度平滑，呈現略帶水彩或塑料質感的柔焦感 | 戒指金屬反光缺乏脆度、車身骨架與車燈內部稜角變鈍 |
| **透明邊界黑邊 (Dark Haloing)** | PNG 去背圖的透明區域 RGB 通常為 `(0, 0, 0)`。常規銳化濾鏡若直接對 RGBA 進行空間卷積，邊界半透明像素會與背景純黑混色，產生深色髒邊 | 花瓣或車身外輪廓浮現一圈黑邊 |

---

## 2. 演算法架構

為徹底解決上述問題，我們在 `crates/ivsr-filters/prototype/enhance_sr_sprites.py` 實作了針對 RGBA Sprite 設計的 ISP 級後處理流程：

```text
[超分 RGBA 圖檔]
       │
       ├─────────────────────────────────┐
       ▼                                 ▼
1. Color Bleed Dilation           5. 保留原始 Alpha
   (將 alpha>180 像素顏色向外平推)             │
       │                                 │
       ▼                                 │
2. 轉至 LAB 空間 (鎖定 A, B 色度通道)       │
       │                                 │
       ▼                                 │
3. 亮度階調對映 (L Channel CDF Match)      │
   - 參考原圖直方圖，拉回暗部下潛與高光動態      │
   - 經 1D 高斯平滑 LUT，保證無階梯斷層       │
       │                                 │
       ▼                                 │
4. 雙頻 ISP Coring 銳化                  │
   - 高頻細節 (Radius 0.85) + 中頻清晰度   │
   - Coring 軟門檻：平滑漸層處抑制噪點       │
   - 邊緣衰減遮罩：輪廓羽化處漸變為 0        │
       │                                 │
       ▼                                 │
6. 轉回 RGB 並套回原始 Alpha 遮罩 ◄────────┘
       │
       ▼
[無黑邊、銳利通透的成果圖]
```

### 關鍵技術亮點

1. **Color Bleed Dilation（色彩外推）**：
   在任何空間濾波前，透過形態學膨脹將物體內部實體顏色平滑延伸至半透明及透明區域，使色彩場在透明邊界處保持連續無崖跳，根本杜絕黑邊。
2. **Smooth Tone Curve Mapping（平滑階調對映）**：
   在 LAB 空間的 L 通道比較超分圖與原圖的累積分布函數（CDF），建立還原查找表（LUT）並施加高斯平滑，精準復原美術原始設計的飽滿對比，且完全不產生色斑或階梯斷層。
3. **ISP Coring（門檻防噪）銳化**：
   借鑒專業相機訊號處理器（ISP）的 Coring 機制。振幅小於門檻值（預設 `2.2`）的微小變動平滑歸零，確保大面積平滑漸層（如車蓋、戒指盒平絨）維持細膩平整，僅對線條、摺線與反光亮點進行銳化。
4. **Silhouette Edge Attenuation（輪廓邊緣衰減保護）**：
   利用侵蝕後的 Alpha 通道建立衰減遮罩，靠近去背輪廓外緣處銳化權重平滑歸零，保留原本乾淨的抗鋸齒邊緣。

---

## 3. 原型腳本使用方式

腳本位於 `crates/ivsr-filters/prototype/enhance_sr_sprites.py`，支援標準 Python 與 `uv`。日常使用請改用第 4 節的原生濾鏡，腳本保留作為對照。

### 依賴安裝

專案內建支援 PEP 723 inline script metadata，若有安裝 `uv` 可直接無縫執行：

```sh
# 方式 A: 透過 uv 直接執行 (自動處理虛擬環境與套件)
uv run crates/ivsr-filters/prototype/enhance_sr_sprites.py --help

# 方式 B: 使用傳統 pip
pip install -r crates/ivsr-filters/prototype/requirements.txt
python3 crates/ivsr-filters/prototype/enhance_sr_sprites.py --help
```

### 單一圖檔處理

```sh
# 有原始圖參考 (強烈推薦: 能精準還原美術設計原本的動態範圍)
uv run crates/ivsr-filters/prototype/enhance_sr_sprites.py \
  Downloads/gift_04.png \
  -r path/to/original/gift_04.png \
  -o Downloads/gift_04_enhanced.png

# 無原始圖參考 (使用內建自適應 S 曲線還原)
uv run crates/ivsr-filters/prototype/enhance_sr_sprites.py \
  Downloads/gift_04.png \
  -o Downloads/gift_04_enhanced.png
```

### 整批目錄處理

```sh
uv run crates/ivsr-filters/prototype/enhance_sr_sprites.py \
  -d path/to/upscaled_dir/ \
  -r path/to/original_dir/ \
  -o path/to/enhanced_dir/
```

### 參數調節指南

| 參數 | 預設值 | 說明與建議 |
| --- | --- | --- |
| `--sharpen` | `1.1` | **高頻微細節強度**。範圍 `0.8 ~ 1.8`。金屬、寶石、硬表面機械可調高至 `1.3~1.5`；柔和角色皮膚可降至 `0.9` |
| `--clarity` | `0.3` | **中頻清晰度強度**。範圍 `0.2 ~ 0.5`。增強主體立體輪廓與雕塑感 |
| `--contrast` | `0.65` | **對比還原權重**。範圍 `0.0 ~ 1.0`。`1.0` 完全拉回原圖階調，`0.5` 保留部分超分平滑 |
| `--coring` | `2.2` | **ISP 軟門檻防噪**。若發現平坦表面有些微雜訊感，可微調至 `2.5~3.0` |
| `--sat` | `1.05` | **飽和度倍率**。超分後色彩常有微量泛白，`1.05` 能提振圖標在遊戲介面中的鮮豔度 |

---

## 4. IVSR 原生整合

此管線已以純 Rust 移植進 `crates/ivsr-filters`，成為 IVSR 前處理 / 後製濾鏡協議（`ivsr_core::Filter`）的內建濾鏡；Python 腳本保留在 `crates/ivsr-filters/prototype/`，作為對照用的原型。原生版本可直接用於圖片與影片，不需要 Python 或 OpenCV。

### 拆分後的步驟

腳本被拆成四個可獨立開關、排序的濾鏡，內建後製順序即為原腳本的流程：

| 順序 | 濾鏡 id | 對應腳本步驟 | 參數（腳本參數） |
| --- | --- | --- | --- |
| 1 | `alpha-bleed` | Color Bleed Dilation | `threshold`（180）、`distance`（24 px） |
| 2 | `tone-restore` | LAB L 通道 CDF 對映 / S 曲線 | `strength`（`--contrast`）、`temporal`（影片跨幀平滑） |
| 3 | `detail-sharpen` | 雙頻 Coring 銳化 + 邊緣衰減 | `amount`（`--sharpen`）、`clarity`（`--clarity`）、`coring`（`--coring`） |
| 4 | `saturation` | A/B 通道飽和度 | `amount`（`--sat`） |

內建前處理只有 `alpha-bleed`（`threshold = 0`）：放大前只替完全透明的像素填色，可見像素不變。以 `realesrgan-x4plus` 放大硬去背 sprite 實測，alpha 1–60 的邊緣像素平均亮度從 74.6 回到 104.0（來源為 104.1），即模型讀到透明區的黑色所造成的深色髒邊。

### 使用方式

```sh
ivsr --post icon.png                                    # 放大後以內建順序後製
ivsr --pre --post=tone-restore,detail-sharpen clip.mp4  # 影片同樣適用, 逐幀以來源幀為參考
ivsr --post -F detail-sharpen.amount=1.4 gems/          # 調整單一步驟的參數
ivsr filters                                            # 列出濾鏡與目前的處理鏈
ivsr filters apply out/icon_x4.png                      # 只套用濾鏡, 原圖自動從歷史紀錄找回
ivsr config set filters.post.enabled true               # 預設開啟後製
```

桌面版在設定面板有「前處理」與「後製」兩區，可開關、排序、調整參數；比較檢視器按 `F` 開啟濾鏡面板，可對成果或來源試套濾鏡、與未處理版本對照、另存檔案，或把這組步驟設為工作流預設。

### 影片

- 參考圖逐幀取自同一幀的來源（經前處理後、送進引擎的那張），所以階調對映每一幀都有精準參考。
- `tone-restore` 會把前一幀的曲線依 `temporal` 比重混入，避免逐幀直方圖造成閃爍；參考幀直方圖差異過大（場景切換）時重新開始。
- 影片沒有 alpha，`alpha-bleed` 與邊緣保護自動略過。

### 與原型的差異

- **色彩外推不再混入黑色**：原型以 `cv2.blur` 平均整個 5×5 視窗，會把透明像素底下的黑色帶進第一圈；原生版只平均已填色或 alpha > 0 的像素。半透明像素（柔光）的顏色仍會參與混合，與原型一致。
- **無參考圖時的 S 曲線改為端點固定**：原型的 tanh 曲線未正規化，本身把純白對到約 230、純黑對到約 25，以預設強度 0.65 混合後白色仍被壓低約 8 階、黑色抬高約 8 階，與「拉回對比」的目的相反；原生版維持 0 與 255 不動，只提高中間調對比。
- **以浮點 Lab 運算**：原型把 Lab 截斷成 8-bit 整數，飽和色在轉回 RGB 時誤差最多 26 階；原生版與 OpenCV 浮點版相差不超過 1 階。
- 參考圖直方圖以來源原尺寸計算，不先放大；完全透明的像素保持原值（不歸零），只由 `alpha-bleed` 改色。

以 Real-ESRGAN x4plus 放大的合成 sprite 比對（有參考圖）：alpha > 180 的像素與原型平均差 1.4 階、99% 在 6 階內；邊緣像素平均差 0.8 階。作為對照，每個步驟本身造成的平均變化約 4–5 階。
