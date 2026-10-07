//! Parsing `ffprobe -print_format json -show_format -show_streams` output.

use serde::Deserialize;

use ivsr_core::{AudioInfo, Error, Result, VideoInfo};

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    nb_frames: Option<String>,
    duration: Option<String>,
    channels: Option<u32>,
    #[serde(default)]
    disposition: Disposition,
    #[serde(default)]
    side_data_list: Vec<SideData>,
    #[serde(default)]
    tags: Tags,
}

#[derive(Deserialize, Default)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

#[derive(Deserialize, Default)]
struct Tags {
    rotate: Option<String>,
}

#[derive(Deserialize)]
struct SideData {
    rotation: Option<f64>,
}

#[derive(Deserialize)]
struct Format {
    format_name: Option<String>,
    duration: Option<String>,
}

fn parse_rate(rate: &str) -> Option<(u32, u32)> {
    let (num, den) = rate.split_once('/').unwrap_or((rate, "1"));
    let (num, den) = (num.trim().parse().ok()?, den.trim().parse().ok()?);
    (num > 0 && den > 0).then_some((num, den))
}

pub(crate) fn parse(json: &str) -> Result<VideoInfo> {
    let probe: Probe = serde_json::from_str(json).map_err(|e| Error::tool("ffprobe", format!("bad output: {e}")))?;
    let video = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video") && s.disposition.attached_pic == 0)
        .ok_or_else(|| Error::UnsupportedFormat("no video stream".into()))?;
    let (mut width, mut height) = match (video.width, video.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
        _ => return Err(Error::UnsupportedFormat("video stream has no dimensions".into())),
    };
    // ffmpeg auto-rotates decoded frames, so extracted frames have swapped sides.
    let rotation = video
        .side_data_list
        .iter()
        .find_map(|d| d.rotation)
        .or_else(|| video.tags.rotate.as_deref().and_then(|r| r.parse().ok()))
        .unwrap_or(0.0);
    if (rotation.abs().round() as i64) % 180 == 90 {
        std::mem::swap(&mut width, &mut height);
    }
    let (fps_num, fps_den) = [&video.avg_frame_rate, &video.r_frame_rate]
        .into_iter()
        .flatten()
        .find_map(|r| parse_rate(r))
        .ok_or_else(|| Error::UnsupportedFormat("video stream has no frame rate".into()))?;
    let duration = video
        .duration
        .as_deref()
        .or(probe.format.as_ref().and_then(|f| f.duration.as_deref()))
        .and_then(|d| d.parse().ok());
    let audio = probe.streams.iter().find(|s| s.codec_type.as_deref() == Some("audio")).map(|s| AudioInfo {
        codec: s.codec_name.clone().unwrap_or_default(),
        channels: s.channels.unwrap_or(0),
    });
    let container = probe
        .format
        .and_then(|f| f.format_name)
        .map(|n| n.split(',').next().unwrap_or_default().to_string())
        .unwrap_or_default();
    Ok(VideoInfo {
        width,
        height,
        fps_num,
        fps_den,
        frame_count: video.nb_frames.as_deref().and_then(|n| n.parse().ok()).filter(|n| *n > 0),
        duration,
        codec: video.codec_name.clone().unwrap_or_default(),
        container,
        audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ntsc_rate_audio_and_rotation() {
        let json = r#"{
          "streams": [
            {"codec_type": "video", "codec_name": "mjpeg", "width": 300, "height": 300,
             "avg_frame_rate": "0/0", "r_frame_rate": "90000/1", "disposition": {"attached_pic": 1}},
            {"codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080,
             "avg_frame_rate": "30000/1001", "r_frame_rate": "30000/1001", "nb_frames": "300",
             "side_data_list": [{"side_data_type": "Display Matrix", "rotation": -90}]},
            {"codec_type": "audio", "codec_name": "aac", "channels": 2}
          ],
          "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "10.010000"}
        }"#;
        let info = parse(json).unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
        assert_eq!((info.fps_num, info.fps_den), (30000, 1001));
        assert_eq!(info.frame_count, Some(300));
        assert_eq!(info.duration, Some(10.01));
        assert_eq!(info.container, "mov");
        assert_eq!(info.audio, Some(AudioInfo { codec: "aac".into(), channels: 2 }));
    }

    #[test]
    fn falls_back_to_r_frame_rate_and_estimates_frames() {
        let json = r#"{"streams": [{"codec_type": "video", "codec_name": "vp9", "width": 640, "height": 360,
            "avg_frame_rate": "0/0", "r_frame_rate": "25/1"}], "format": {"format_name": "matroska,webm", "duration": "4.0"}}"#;
        let info = parse(json).unwrap();
        assert_eq!((info.fps_num, info.fps_den, info.frame_count), (25, 1, None));
        assert_eq!(info.estimated_frames(), 100);
    }

    #[test]
    fn audio_only_file_is_not_a_video() {
        let json = r#"{"streams": [{"codec_type": "audio", "codec_name": "mp3"}], "format": {}}"#;
        assert!(matches!(parse(json), Err(Error::UnsupportedFormat(_))));
    }
}
