#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "opencv-python>=4.8.0",
#     "pillow>=10.0.0",
#     "scipy>=1.11.0",
#     "numpy>=1.24.0",
# ]
# ///
"""
enhance_sr_sprites.py

針對 AI 超解析度 (Super-Resolution, 如 Real-ESRGAN) 放大遊戲圖標 / 去背 Sprite 後，
可能產生的「對比度降低、細節微對比平淡、暗部發灰、高光被壓低」等問題所設計的專門後處理工具。

主要特色：
1. Alpha-Safe Anti-Halo（去背邊緣色彩外推）：
   在濾波前將實體像素顏色向外平滑填充，處理後套回原始 Alpha，徹底消除常規銳化在去背邊緣產生的黑邊（Dark Halo）或階梯斷層。
2. LAB 空間平滑階調對比還原（Smooth Tone Curve Mapping）：
   在 LAB 空間的 L（亮度）通道上，參考低解析度原圖進行平滑化 CDF 對映，還原美術設計原本的動態範圍與暗部深邃感。
3. ISP 級 Coring 門檻防噪銳化（Thresholded USM & Clarity）：
   借鑒攝影機訊號處理（ISP）的 Coring 技術，在平滑漸層區（如平滑盒面、車蓋）抑制雜訊放大，僅在結構線與反光高光點加強微對比。
4. 輪廓外緣保護（Silhouette Edge Attenuation）：
   對靠近 Alpha 羽化區的邊緣進行平滑衰減，保持透明邊界滑順抗鋸齒。
"""

import argparse
import sys
from pathlib import Path
from typing import Optional

import cv2
import numpy as np
from PIL import Image
from scipy.ndimage import gaussian_filter1d


def soft_coring(detail: np.ndarray, threshold: float = 2.2) -> np.ndarray:
    """ISP 級軟門檻衰減：低於 threshold 的微小振幅（雜訊）平滑衰減至 0，避免平滑漸層處產生噪點。"""
    mag = np.abs(detail)
    weight = np.clip((mag - threshold) / max(threshold, 1e-4), 0.0, 1.0)
    # Smoothstep: 3x^2 - 2x^3
    weight = weight * weight * (3.0 - 2.0 * weight)
    return detail * weight


def dilate_color_bleed(rgb: np.ndarray, alpha: np.ndarray, iterations: int = 12) -> np.ndarray:
    """將實體區域 (alpha > 180) 的顏色向半透明與透明區域外推，消除透明黑底與濾波核混合導致的邊緣黑邊。"""
    solid_mask = (alpha > 180).astype(np.uint8)
    kernel = cv2.getStructuringElement(cv2.MORPH_RECT, (5, 5))
    dilated_mask = solid_mask.copy()
    dilated_rgb = rgb.copy()
    for _ in range(iterations):
        new_mask = cv2.dilate(dilated_mask, kernel)
        frontier = (new_mask == 1) & (dilated_mask == 0)
        if not np.any(frontier):
            break
        blurred = cv2.blur(dilated_rgb, (5, 5))
        dilated_rgb[frontier] = blurred[frontier]
        dilated_mask = new_mask
    return np.where(alpha[..., None] > 180, rgb, dilated_rgb)


def enhance_sprite(
    up_path: Path,
    orig_path: Optional[Path] = None,
    sharpen_strength: float = 1.1,
    clarity_strength: float = 0.3,
    contrast_weight: float = 0.65,
    sat_boost: float = 1.05,
    coring_threshold: float = 2.2,
) -> Image.Image:
    """處理單張 Sprite 圖檔，回傳強化後的 PIL RGBA Image。"""
    up_img = Image.open(up_path).convert("RGBA")
    up_arr = np.array(up_img)
    rgb = up_arr[:, :, :3].astype(np.float32)
    alpha = up_arr[:, :, 3].astype(np.float32)

    # 1. 邊緣外推防止輪廓黑邊
    clean_rgb = dilate_color_bleed(rgb, alpha)
    lab = cv2.cvtColor(clean_rgb.astype(np.uint8), cv2.COLOR_RGB2LAB).astype(np.float32)
    L, A, B = lab[:, :, 0], lab[:, :, 1], lab[:, :, 2]
    m_up = alpha > 120

    # 2. 階調與對比還原 (LAB L-channel)
    if orig_path and Path(orig_path).is_file():
        orig_img = Image.open(orig_path).convert("RGBA")
        orig_resized = np.array(orig_img.resize(up_img.size, Image.Resampling.LANCZOS))
        orig_alpha = orig_resized[:, :, 3].astype(np.float32)
        orig_lab = cv2.cvtColor(orig_resized[:, :, :3], cv2.COLOR_RGB2LAB).astype(np.float32)
        orig_L = orig_lab[:, :, 0]
        m_orig = orig_alpha > 120

        s_vals = L[m_up]
        t_vals = orig_L[m_orig]
        s_hist, _ = np.histogram(s_vals, bins=256, range=(0, 256))
        t_hist, _ = np.histogram(t_vals, bins=256, range=(0, 256))
        s_cdf = np.cumsum(s_hist).astype(np.float32) / max(1, s_hist.sum())
        t_cdf = np.cumsum(t_hist).astype(np.float32) / max(1, t_hist.sum())

        lut = np.zeros(256, dtype=np.float32)
        for i in range(256):
            lut[i] = np.interp(s_cdf[i], t_cdf, np.arange(256))

        # 1D 高斯平滑 LUT，消除色階斷層與量化雜訊
        lut_smooth = gaussian_filter1d(lut, sigma=4.0)
        matched_L = L.copy()
        matched_L[m_up] = np.interp(s_vals, np.arange(256), lut_smooth)
        L_base = (1.0 - contrast_weight) * L + contrast_weight * matched_L
    else:
        # 無參考圖時使用自適應雙曲正切 S 曲線
        L_norm = L / 255.0
        s_curve = 0.5 * (1.0 + np.tanh(2.2 * (L_norm - 0.5)))
        L_base = (1.0 - contrast_weight * 0.5) * L + (contrast_weight * 0.5) * (s_curve * 255.0)

    # 3. 雙頻分離銳化 (高頻細節 + 中頻清晰度)
    blur_fine = cv2.GaussianBlur(L_base, (0, 0), 0.85)
    high_freq = soft_coring(L_base - blur_fine, threshold=coring_threshold)

    blur_med = cv2.GaussianBlur(L_base, (0, 0), 2.5)
    mid_freq = soft_coring(np.clip(L_base - blur_med, -20, 20), threshold=coring_threshold * 0.7)

    # 邊緣遮罩：保護透明羽化邊緣
    interior_mask = cv2.erode((alpha > 150).astype(np.float32), cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (5, 5)))
    sharp_weight = cv2.GaussianBlur(interior_mask, (0, 0), 1.5)

    L_sharp = L_base + (sharpen_strength * high_freq + clarity_strength * mid_freq) * sharp_weight
    L_final = np.clip(L_sharp, 0, 255)

    # 4. 微幅提亮飽和度 (還原原畫飽和鮮豔度)
    if sat_boost != 1.0:
        A = np.clip((A - 128.0) * sat_boost + 128.0, 0, 255)
        B = np.clip((B - 128.0) * sat_boost + 128.0, 0, 255)

    lab_out = np.stack([L_final, A, B], axis=-1).astype(np.uint8)
    rgb_out = cv2.cvtColor(lab_out, cv2.COLOR_LAB2RGB)

    # 5. 重新套回原始 Alpha 遮罩
    out = np.zeros((up_arr.shape[0], up_arr.shape[1], 4), dtype=np.uint8)
    out[:, :, :3] = np.where(up_arr[:, :, 3:] > 0, rgb_out, 0)
    out[:, :, 3] = up_arr[:, :, 3]
    return Image.fromarray(out)


def main():
    parser = argparse.ArgumentParser(
        description="IVSR 後處理工具：針對超分放大後的 Sprite 圖標進行對比度、銳利度與階調還原"
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("input", nargs="?", type=Path, help="單一超分圖檔路徑 (RGBA PNG)")
    group.add_argument("-d", "--dir", type=Path, help="批次處理目錄 (包含超分圖檔的資料夾)")

    parser.add_argument("-r", "--reference", type=Path, help="原始低解析度圖檔或目錄 (用於精準階調與對比還原)")
    parser.add_argument("-o", "--output", type=Path, required=True, help="輸出圖檔路徑或輸出目錄")
    parser.add_argument("--sharpen", type=float, default=1.1, help="高頻微細節銳化強度 (預設: 1.1)")
    parser.add_argument("--clarity", type=float, default=0.3, help="中頻清晰度強度 (預設: 0.3)")
    parser.add_argument("--contrast", type=float, default=0.65, help="對比還原混合權重 0~1 (預設: 0.65)")
    parser.add_argument("--sat", type=float, default=1.05, help="飽和度微調倍率 (預設: 1.05)")
    parser.add_argument("--coring", type=float, default=2.2, help="ISP 防噪門檻值 (預設: 2.2)")

    args = parser.parse_args()

    # 批次目錄模式
    if args.dir:
        in_dir = args.dir
        if not in_dir.is_dir():
            print(f"錯誤: 輸入目錄不存在: {in_dir}", file=sys.stderr)
            sys.exit(1)
        out_dir = args.output
        out_dir.mkdir(parents=True, exist_ok=True)

        ref_dir = args.reference if (args.reference and args.reference.is_dir()) else None

        png_files = sorted(in_dir.glob("*.png"))
        if not png_files:
            print(f"提示: 目錄 {in_dir} 下未找到 PNG 圖檔")
            return

        print(f"開始批次處理 {len(png_files)} 個圖檔...")
        for p in png_files:
            ref_path = (ref_dir / p.name) if (ref_dir and (ref_dir / p.name).is_file()) else None
            out_path = out_dir / p.name
            try:
                res = enhance_sprite(
                    p,
                    orig_path=ref_path,
                    sharpen_strength=args.sharpen,
                    clarity_strength=args.clarity,
                    contrast_weight=args.contrast,
                    sat_boost=args.sat,
                    coring_threshold=args.coring,
                )
                res.save(out_path)
                ref_msg = f" (參考: {ref_path.name})" if ref_path else " (無參考圖)"
                print(f"✓ {p.name} → {out_path}{ref_msg}")
            except Exception as e:
                print(f"✗ 處理 {p.name} 失敗: {e}", file=sys.stderr)
        print("批次處理完畢！")
        return

    # 單一檔案模式
    if not args.input.is_file():
        print(f"錯誤: 輸入檔案不存在: {args.input}", file=sys.stderr)
        sys.exit(1)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    res = enhance_sprite(
        args.input,
        orig_path=args.reference,
        sharpen_strength=args.sharpen,
        clarity_strength=args.clarity,
        contrast_weight=args.contrast,
        sat_boost=args.sat,
        coring_threshold=args.coring,
    )
    res.save(args.output)
    print(f"✓ 處理完成: {args.output}")


if __name__ == "__main__":
    main()
