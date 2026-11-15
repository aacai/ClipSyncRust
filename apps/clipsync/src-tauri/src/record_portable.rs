//! Windows/Linux 录屏：xcap 抓帧，openh264 编码，mp4 封装。

use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use image::imageops::FilterType;
use mp4::{AvcConfig, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig, TrackType};
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, Profile, RateControlMode,
    UsageType,
};
use openh264::formats::{RgbaSliceU8, YUVBuffer};
use openh264::OpenH264API;

pub struct PortableSpec {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub fps: i32,
    pub long_edge: i32,
    pub path: PathBuf,
}

struct Active {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Result<(), String>>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

struct Prepared {
    monitor: xcap::Monitor,
    region: (u32, u32, u32, u32),
    target: (u32, u32),
    fps: u32,
}

pub fn start(spec: PortableSpec) -> Result<(), String> {
    let mut slot = ACTIVE.lock().map_err(|_| "录屏状态不可用".to_string())?;
    if slot.is_some() {
        return Err("已经在录了".to_string());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("clipsync-portable-record".into())
        .spawn(move || run(spec, thread_stop, ready_tx))
        .map_err(|error| format!("录屏线程启动失败：{error}"))?;
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {
            *slot = Some(Active { stop, handle });
            Ok(())
        }
        Ok(Err(error)) => {
            let _ = handle.join();
            Err(error)
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            let _ = handle.join();
            Err("录屏启动超时".to_string())
        }
    }
}

pub fn stop() -> Result<(), String> {
    let active = ACTIVE.lock().ok().and_then(|mut slot| slot.take());
    let Some(active) = active else {
        return Ok(());
    };
    active.stop.store(true, Ordering::SeqCst);
    active
        .handle
        .join()
        .map_err(|_| "录屏线程异常退出".to_string())?
}

fn run(
    spec: PortableSpec,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    let prepared = match prepare(&spec) {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    let result = session(&spec, &prepared, &stop, &ready);
    if let Err(error) = &result {
        // 启动阶段没发出 ready 的话，这里再补一次，主线程不会一直等到超时。
        let _ = ready.send(Err(error.clone()));
    }
    result
}

fn prepare(spec: &PortableSpec) -> Result<Prepared, String> {
    let monitor = crate::screenshot::primary_monitor()?;
    let probe = monitor
        .capture_image()
        .map_err(|error| format!("读取屏幕失败：{error}"))?;
    let monitor_width = monitor
        .width()
        .map_err(|error| format!("读取屏幕尺寸失败：{error}"))?;
    let scale = probe.width() as f64 / f64::from(monitor_width.max(1));
    let (full_w, full_h) = (probe.width(), probe.height());
    let region = if spec.w >= 2.0 && spec.h >= 2.0 {
        clamp_region(spec, scale, full_w, full_h)
    } else {
        (0, 0, full_w, full_h)
    };
    let target = target_size(region.2, region.3, spec.long_edge);
    Ok(Prepared {
        monitor,
        region,
        target,
        fps: spec.fps.clamp(5, 60) as u32,
    })
}

fn clamp_region(spec: &PortableSpec, scale: f64, full_w: u32, full_h: u32) -> (u32, u32, u32, u32) {
    let x = (spec.x * scale).round().max(0.0) as u32;
    let y = (spec.y * scale).round().max(0.0) as u32;
    let w = (spec.w * scale).round() as u32;
    let h = (spec.h * scale).round() as u32;
    let x = x.min(full_w.saturating_sub(2));
    let y = y.min(full_h.saturating_sub(2));
    let w = w.min(full_w - x).max(2);
    let h = h.min(full_h - y).max(2);
    (x, y, w, h)
}

fn target_size(width: u32, height: u32, long_edge: i32) -> (u32, u32) {
    let (mut w, mut h) = (width, height);
    if long_edge > 0 && w.max(h) > long_edge as u32 {
        let edge = long_edge as f64;
        if w >= h {
            h = (f64::from(h) * edge / f64::from(w)).round() as u32;
            w = long_edge as u32;
        } else {
            w = (f64::from(w) * edge / f64::from(h)).round() as u32;
            h = long_edge as u32;
        }
    }
    (even(w), even(h))
}

fn even(value: u32) -> u32 {
    (value / 2 * 2).max(16)
}

fn session(
    spec: &PortableSpec,
    prepared: &Prepared,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    let (width, height) = prepared.target;
    let bitrate = (u64::from(width) * u64::from(height) * 6).clamp(2_000_000, 40_000_000) as u32;
    let config = EncoderConfig::new()
        .usage_type(UsageType::ScreenContentRealTime)
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(bitrate))
        .max_frame_rate(FrameRate::from_hz(prepared.fps as f32))
        .skip_frames(false)
        .intra_frame_period(IntraFramePeriod::from_num_frames(prepared.fps * 2))
        .profile(Profile::Baseline)
        .num_threads(0);
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
        .map_err(|error| format!("H.264 编码器初始化失败：{error}"))?;

    let first_at = Instant::now();
    let first = capture(prepared)?;
    let first = encode(&mut encoder, &first, width, height)?;
    let (sps, pps) = first
        .parameter_sets
        .ok_or_else(|| "编码器没有输出 SPS/PPS".to_string())?;

    let path = &spec.path;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建录屏目录失败：{error}"))?;
    }
    let file = File::create(path).map_err(|error| format!("创建录屏文件失败：{error}"))?;
    let mp4_config = Mp4Config {
        major_brand: "isom".parse().map_err(|_| "MP4 品牌无效".to_string())?,
        minor_version: 512,
        compatible_brands: vec![
            "isom".parse().map_err(|_| "MP4 品牌无效".to_string())?,
            "iso2".parse().map_err(|_| "MP4 品牌无效".to_string())?,
            "avc1".parse().map_err(|_| "MP4 品牌无效".to_string())?,
            "mp41".parse().map_err(|_| "MP4 品牌无效".to_string())?,
        ],
        timescale: 1000,
    };
    let mut writer = Mp4Writer::write_start(file, &mp4_config)
        .map_err(|error| format!("创建 MP4 失败：{error}"))?;
    writer
        .add_track(&TrackConfig {
            track_type: TrackType::Video,
            timescale: 1000,
            language: "und".to_string(),
            media_conf: MediaConfig::AvcConfig(AvcConfig {
                width: width as u16,
                height: height as u16,
                seq_param_set: sps,
                pic_param_set: pps,
            }),
        })
        .map_err(|error| format!("写入视频轨道失败：{error}"))?;

    let _ = ready.send(Ok(()));

    let frame_interval = Duration::from_millis(u64::from(1000 / prepared.fps));
    let nominal_ms = (1000 / prepared.fps).max(1);
    let mut pending: Option<(Vec<u8>, bool, Instant)> = Some((first.bytes, first.sync, first_at));
    let mut next_deadline = Instant::now() + frame_interval;
    let mut start_time = 0u64;
    let mut loop_error: Option<String> = None;

    while !stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        if now < next_deadline {
            thread::sleep(next_deadline - now);
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        next_deadline = Instant::now() + frame_interval;
        let captured_at = Instant::now();
        let image = match capture(prepared) {
            Ok(image) => image,
            Err(error) => {
                loop_error = Some(error);
                break;
            }
        };
        let Encoded { bytes, sync, .. } = match encode(&mut encoder, &image, width, height) {
            Ok(encoded) => encoded,
            Err(error) => {
                loop_error = Some(error);
                break;
            }
        };
        if let Some((bytes, sync, at)) = pending.take() {
            let duration = (captured_at - at).as_millis().max(1) as u32;
            if let Err(error) = write_sample(&mut writer, &mut start_time, bytes, sync, duration) {
                loop_error = Some(error);
                break;
            }
        }
        pending = Some((bytes, sync, captured_at));
    }

    if let Some((bytes, sync, _)) = pending.take() {
        if let Err(error) = write_sample(&mut writer, &mut start_time, bytes, sync, nominal_ms) {
            loop_error.get_or_insert(error);
        }
    }
    if let Err(error) = writer.write_end() {
        loop_error.get_or_insert(format!("收尾写入失败：{error}"));
    }
    match loop_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn capture(prepared: &Prepared) -> Result<image::RgbaImage, String> {
    let (x, y, w, h) = prepared.region;
    let image = prepared
        .monitor
        .capture_region(x, y, w, h)
        .map_err(|error| format!("抓取屏幕失败：{error}"))?;
    if (image.width(), image.height()) == prepared.target {
        return Ok(image);
    }
    Ok(image::imageops::resize(
        &image,
        prepared.target.0,
        prepared.target.1,
        FilterType::Triangle,
    ))
}

struct Encoded {
    bytes: Vec<u8>,
    sync: bool,
    parameter_sets: Option<(Vec<u8>, Vec<u8>)>,
}

fn encode(
    encoder: &mut Encoder,
    image: &image::RgbaImage,
    width: u32,
    height: u32,
) -> Result<Encoded, String> {
    let source = RgbaSliceU8::new(image.as_raw(), (width as usize, height as usize));
    let yuv = YUVBuffer::from_rgba8_source(source);
    let stream = encoder
        .encode(&yuv)
        .map_err(|error| format!("H.264 编码失败：{error}"))?;
    let mut bytes = Vec::new();
    let mut parameter_sets = None;
    for index in 0..stream.num_layers() {
        let Some(layer) = stream.layer(index) else {
            continue;
        };
        for unit in 0..layer.nal_count() {
            let Some(nal) = layer.nal_unit(unit) else {
                continue;
            };
            let nal = strip_start_code(nal);
            if nal.is_empty() {
                continue;
            }
            match (layer.is_video(), nal[0] & 0x1F) {
                (true, _) => {
                    bytes.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                    bytes.extend_from_slice(nal);
                }
                (false, 7) => {
                    parameter_sets
                        .get_or_insert_with(|| (Vec::new(), Vec::new()))
                        .0 = nal.to_vec();
                }
                (false, 8) => {
                    parameter_sets
                        .get_or_insert_with(|| (Vec::new(), Vec::new()))
                        .1 = nal.to_vec();
                }
                _ => {}
            }
        }
    }
    // 第一帧之后每次都可能带一套参数集，只有第一次需要塞进 avcC。
    let parameter_sets = parameter_sets.filter(|(sps, pps)| !sps.is_empty() && !pps.is_empty());
    Ok(Encoded {
        bytes,
        sync: stream.frame_type() == openh264::encoder::FrameType::IDR,
        parameter_sets,
    })
}

fn strip_start_code(nal: &[u8]) -> &[u8] {
    if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        nal
    }
}

fn write_sample(
    writer: &mut Mp4Writer<File>,
    start_time: &mut u64,
    bytes: Vec<u8>,
    sync: bool,
    duration: u32,
) -> Result<(), String> {
    if bytes.is_empty() {
        return Ok(());
    }
    writer
        .write_sample(
            1,
            &Mp4Sample {
                start_time: *start_time,
                duration,
                rendering_offset: 0,
                is_sync: sync,
                bytes: bytes.into(),
            },
        )
        .map_err(|error| format!("写入 MP4 帧失败：{error}"))?;
    *start_time += u64::from(duration);
    Ok(())
}
