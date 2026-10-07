//! Video codec and container catalogue, and the ffmpeg arguments each implies.

use std::collections::HashSet;

use ivsr_core::{AudioInfo, AudioMode, CodecInfo, Error, QualityRange, Result, VideoEncodeOptions};

pub(crate) struct Codec {
    pub id: &'static str,
    pub label: &'static str,
    pub encoder: &'static str,
    pub containers: &'static [&'static str],
    /// (min, max, default) for the quality flag; lower is better.
    pub quality: Option<(u32, u32, u32)>,
    pub quality_flag: &'static str,
    pub presets: &'static [&'static str],
    pub default_preset: Option<&'static str>,
    pub pix_fmt: &'static str,
    pub extra: &'static [&'static str],
}

const X26X_PRESETS: &[&str] =
    &["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];
const SVT_PRESETS: &[&str] = &["4", "6", "8", "10", "12"];

pub(crate) const CODECS: &[Codec] = &[
    Codec {
        id: "h264",
        label: "H.264 / AVC",
        encoder: "libx264",
        containers: &["mp4", "mkv", "mov"],
        quality: Some((0, 51, 18)),
        quality_flag: "-crf",
        presets: X26X_PRESETS,
        default_preset: Some("medium"),
        pix_fmt: "yuv420p",
        extra: &[],
    },
    Codec {
        id: "h265",
        label: "H.265 / HEVC",
        encoder: "libx265",
        containers: &["mp4", "mkv", "mov"],
        quality: Some((0, 51, 20)),
        quality_flag: "-crf",
        presets: X26X_PRESETS,
        default_preset: Some("medium"),
        pix_fmt: "yuv420p",
        // `hvc1` lets QuickTime and Apple devices play the file.
        extra: &["-tag:v", "hvc1", "-x265-params", "log-level=error"],
    },
    Codec {
        id: "av1",
        label: "AV1 (SVT-AV1)",
        encoder: "libsvtav1",
        containers: &["mp4", "mkv", "webm"],
        quality: Some((0, 63, 30)),
        quality_flag: "-crf",
        presets: SVT_PRESETS,
        default_preset: Some("8"),
        pix_fmt: "yuv420p",
        extra: &[],
    },
    Codec {
        id: "vp9",
        label: "VP9",
        encoder: "libvpx-vp9",
        containers: &["webm", "mkv", "mp4"],
        quality: Some((0, 63, 30)),
        quality_flag: "-crf",
        presets: &[],
        default_preset: None,
        pix_fmt: "yuv420p",
        extra: &["-b:v", "0", "-row-mt", "1", "-deadline", "good", "-cpu-used", "2"],
    },
    Codec {
        id: "prores",
        label: "Apple ProRes 422 HQ",
        encoder: "prores_ks",
        containers: &["mov", "mkv"],
        quality: None,
        quality_flag: "",
        presets: &[],
        default_preset: None,
        pix_fmt: "yuv422p10le",
        extra: &["-profile:v", "3", "-vendor", "apl0"],
    },
];

pub(crate) fn find(id: &str) -> Option<&'static Codec> {
    CODECS.iter().find(|c| c.id == id)
}

pub(crate) fn catalogue(encoders: &HashSet<String>) -> Vec<CodecInfo> {
    CODECS
        .iter()
        .map(|c| CodecInfo {
            id: c.id.into(),
            label: c.label.into(),
            containers: c.containers.iter().map(|s| s.to_string()).collect(),
            quality: c.quality.map(|(min, max, default)| QualityRange { label: "CRF".into(), min, max, default }),
            presets: c.presets.iter().map(|s| s.to_string()).collect(),
            default_preset: c.default_preset.map(Into::into),
            hardware: false,
            available: encoders.contains(c.encoder),
        })
        .collect()
}

/// Output containers: (id, label, extensions, encodable).
pub(crate) const CONTAINERS: &[(&str, &str, &[&str], bool)] = &[
    ("mp4", "MP4", &["mp4", "m4v"], true),
    ("mkv", "Matroska", &["mkv"], true),
    ("mov", "QuickTime", &["mov"], true),
    ("webm", "WebM", &["webm"], true),
    ("avi", "AVI", &["avi"], false),
    ("flv", "Flash Video", &["flv"], false),
    ("ts", "MPEG-TS", &["ts", "m2ts", "mts"], false),
    ("mpeg", "MPEG-PS", &["mpg", "mpeg", "vob"], false),
    ("wmv", "Windows Media", &["wmv", "asf"], false),
    ("3gp", "3GPP", &["3gp", "3g2"], false),
];

pub(crate) fn container_of(ext: &str) -> Option<&'static str> {
    CONTAINERS.iter().find(|(_, _, exts, _)| exts.contains(&ext)).map(|(id, ..)| *id)
}

/// Video encoder arguments for `opts`, validated against `container`.
pub(crate) fn video_args(opts: &VideoEncodeOptions, container: &str) -> Result<Vec<String>> {
    let codec = find(&opts.codec).ok_or_else(|| Error::UnsupportedFormat(format!("unknown video codec `{}`", opts.codec)))?;
    if !codec.containers.contains(&container) {
        return Err(Error::UnsupportedFormat(format!(
            "{} cannot be stored in .{container} (use one of: {})",
            codec.label,
            codec.containers.join(", ")
        )));
    }
    let mut args: Vec<String> = vec!["-c:v".into(), codec.encoder.into()];
    if let Some((min, max, default)) = codec.quality {
        let q = opts.quality.unwrap_or(default);
        if !(min..=max).contains(&q) {
            return Err(Error::InvalidParam { key: "quality".into(), reason: format!("{q} is outside [{min}, {max}]") });
        }
        args.extend([codec.quality_flag.to_string(), q.to_string()]);
    }
    if let Some(preset) = opts.preset.as_deref().or(codec.default_preset) {
        if !codec.presets.contains(&preset) {
            return Err(Error::InvalidParam {
                key: "preset".into(),
                reason: format!("`{preset}` is not one of: {}", codec.presets.join(", ")),
            });
        }
        args.extend(["-preset".to_string(), preset.to_string()]);
    }
    args.extend(["-pix_fmt".to_string(), codec.pix_fmt.to_string()]);
    args.extend(codec.extra.iter().map(|s| s.to_string()));
    if matches!(container, "mp4" | "mov") {
        args.extend(["-movflags".to_string(), "+faststart".to_string()]);
    }
    Ok(args)
}

fn copyable(container: &str, codec: &str) -> bool {
    match container {
        "mp4" | "mov" => matches!(codec, "aac" | "mp3" | "alac" | "ac3" | "eac3"),
        "webm" => matches!(codec, "opus" | "vorbis"),
        _ => true,
    }
}

/// Audio arguments, or `None` when the output should carry no audio.
pub(crate) fn audio_args(mode: AudioMode, source: Option<&AudioInfo>, container: &str) -> Option<Vec<String>> {
    let source = source?;
    let reencode = || -> Vec<String> {
        let (codec, bitrate) = if container == "webm" { ("libopus", "160k") } else { ("aac", "192k") };
        ["-c:a", codec, "-b:a", bitrate].iter().map(|s| s.to_string()).collect()
    };
    let copy = || vec!["-c:a".to_string(), "copy".to_string()];
    match mode {
        AudioMode::Drop => None,
        AudioMode::Copy => Some(copy()),
        AudioMode::Reencode => Some(reencode()),
        AudioMode::Auto if copyable(container, &source.codec) => Some(copy()),
        AudioMode::Auto => Some(reencode()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(codec: &str) -> VideoEncodeOptions {
        VideoEncodeOptions { codec: codec.into(), quality: None, preset: None, audio: AudioMode::Auto }
    }

    #[test]
    fn h264_defaults_produce_crf_preset_and_faststart() {
        let args = video_args(&opts("h264"), "mp4").unwrap().join(" ");
        assert_eq!(args, "-c:v libx264 -crf 18 -preset medium -pix_fmt yuv420p -movflags +faststart");
    }

    #[test]
    fn codec_container_mismatch_is_rejected() {
        assert!(video_args(&opts("h264"), "webm").is_err());
        assert!(video_args(&opts("prores"), "mp4").is_err());
    }

    #[test]
    fn out_of_range_quality_and_unknown_preset_are_rejected() {
        let mut o = opts("h264");
        o.quality = Some(60);
        assert!(video_args(&o, "mp4").is_err());
        let mut o = opts("h264");
        o.preset = Some("ludicrous".into());
        assert!(video_args(&o, "mkv").is_err());
    }

    #[test]
    fn auto_audio_copies_compatible_and_reencodes_the_rest() {
        let pcm = AudioInfo { codec: "pcm_s16le".into(), channels: 2 };
        let aac = AudioInfo { codec: "aac".into(), channels: 2 };
        assert_eq!(audio_args(AudioMode::Auto, Some(&aac), "mp4").unwrap().join(" "), "-c:a copy");
        assert_eq!(audio_args(AudioMode::Auto, Some(&pcm), "mp4").unwrap().join(" "), "-c:a aac -b:a 192k");
        assert_eq!(audio_args(AudioMode::Auto, Some(&pcm), "mkv").unwrap().join(" "), "-c:a copy");
        assert_eq!(audio_args(AudioMode::Auto, Some(&aac), "webm").unwrap().join(" "), "-c:a libopus -b:a 160k");
        assert!(audio_args(AudioMode::Drop, Some(&aac), "mp4").is_none());
        assert!(audio_args(AudioMode::Copy, None, "mp4").is_none());
    }
}
