//! macOS 录屏：链接 `native/record.m` 里的 ScreenCaptureKit。其它平台直接拒绝。

use std::ffi::{CStr, CString};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn cs_record_start(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        mic: i32,
        system_audio: i32,
        fps: i32,
        long_edge: i32,
        mic_device: *const i8,
        path: *const i8,
    ) -> i32;
    fn cs_record_stop() -> i32;
    fn cs_record_exclude_self() -> i32;
    fn cs_record_inputs() -> *const i8;
    fn cs_record_last_error() -> *const i8;
}

/// 一块可选的麦克风。`id` 是 AVCaptureDevice.uniqueID，直接交给 ScreenCaptureKit。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicInput {
    pub id: String,
    pub name: String,
}

/// 开录参数：坐标是主屏左上角为原点的点，`w`/`h` 小于等于 1 时录整个主屏。
#[derive(Clone, Debug)]
pub struct StartSpec {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub mic: bool,
    pub system_audio: bool,
    pub fps: i32,
    pub long_edge: i32,
    /// 空串表示系统默认输入设备。
    pub mic_device: String,
    pub path: PathBuf,
}

pub enum Job {
    Start(StartSpec),
    Exclude,
    /// 停止后由工作线程收尾：主线程既不碰信号量也不碰几百 MB 的字节。
    Stop(PathBuf),
}

#[derive(Debug)]
pub enum Event {
    Started(Result<(), String>),
    Excluded(Result<(), String>),
    /// 录像留在盘上：回信只给路径、大小和「有没有写完 moov」。
    Stopped {
        error: Option<String>,
        path: PathBuf,
        size: i64,
        finished: bool,
    },
}

/// ScreenCaptureKit 的调用一律离开主线程。
///
/// 主线程等它的回调只能泵 NSRunLoop，而 winit 的 ObjC 事件回调里重入 runloop 就是
/// `panic_cannot_unwind`：进程带着空栈被干掉，连一条错误都留不下。回调本身投到主队列，
/// 所以主线程照常转圈就能送达，工作线程只管阻塞。
pub struct Worker {
    jobs: Sender<Job>,
    events: Receiver<Event>,
}

impl Worker {
    pub fn spawn() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        thread::Builder::new()
            .name("clipsync-record".into())
            .spawn(move || run_worker(jobs_rx, events_tx))
            .ok();
        Self {
            jobs: jobs_tx,
            events: events_rx,
        }
    }

    pub fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

fn run_worker(jobs: Receiver<Job>, events: Sender<Event>) {
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Start(spec) => {
                let result = start(
                    spec.x,
                    spec.y,
                    spec.w,
                    spec.h,
                    spec.mic,
                    spec.system_audio,
                    spec.fps,
                    spec.long_edge,
                    &spec.mic_device,
                    &spec.path,
                );
                let _ = events.send(Event::Started(result));
            }
            Job::Exclude => {
                let _ = events.send(Event::Excluded(exclude_self()));
            }
            Job::Stop(path) => {
                let error = stop().err();
                let size = std::fs::metadata(&path).map_or(0, |meta| meta.len() as i64);
                let finished = size > 0 && is_finished(&path);
                let _ = events.send(Event::Stopped {
                    error,
                    path,
                    size,
                    finished,
                });
            }
        }
    }
}

/// `w`/`h` 小于等于 1 时录整个主屏。坐标是主屏左上角为原点的点。
/// `mic_device` 为空时用系统默认输入。
pub fn start(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    mic: bool,
    system_audio: bool,
    fps: i32,
    long_edge: i32,
    mic_device: &str,
    path: &Path,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let text = path.to_string_lossy();
        let c = CString::new(text.as_ref()).map_err(|_| "路径里有空字节".to_string())?;
        // 设备可能已经被拔掉了：存着的 id 不在当前列表里就退回系统默认，别让开录直接失败。
        let device =
            if mic && !mic_device.is_empty() && !inputs().iter().any(|i| i.id == mic_device) {
                String::new()
            } else {
                mic_device.to_string()
            };
        let device = CString::new(device).map_err(|_| "设备名里有空字节".to_string())?;
        let code = unsafe {
            cs_record_start(
                x,
                y,
                w,
                h,
                i32::from(mic),
                i32::from(system_audio),
                fps,
                long_edge,
                device.as_ptr(),
                c.as_ptr(),
            )
        };
        if code == 0 {
            Ok(())
        } else {
            Err(last_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (
            x,
            y,
            w,
            h,
            mic,
            system_audio,
            fps,
            long_edge,
            mic_device,
            path,
        );
        Err("这个平台不能录屏".to_string())
    }
}

/// 本机可用的麦克风。非 macOS 返回空表，设置页据此只显示「系统默认」。
pub fn inputs() -> Vec<MicInput> {
    #[cfg(target_os = "macos")]
    {
        let ptr = unsafe { cs_record_inputs() };
        if ptr.is_null() {
            return Vec::new();
        }
        let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
        parsed
            .iter()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?.trim();
                if id.is_empty() {
                    return None;
                }
                Some(MicInput {
                    id: id.to_string(),
                    name: item
                        .get("name")
                        .and_then(|name| name.as_str())
                        .unwrap_or(id)
                        .to_string(),
                })
            })
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

pub fn stop() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let code = unsafe { cs_record_stop() };
        if code == 0 {
            Ok(())
        } else {
            Err(last_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// 重新排除自己的窗口。录制条是开录之后才出现的，不在开录时的窗口快照里，
/// 不重刷内容过滤器就会连它一起录进去。
pub fn exclude_self() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if unsafe { cs_record_exclude_self() } == 0 {
            Ok(())
        } else {
            Err(last_error())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// MP4 顶层 box 里必须有 moov 才算收尾完成；没有的就是半成品。
/// 逐块读会误判 box 内部的同名标签，所以要按顶层 box 的头跳着走。
pub fn is_finished(path: &Path) -> bool {
    std::fs::File::open(path).is_ok_and(|mut file| has_moov(&mut file))
}

fn has_moov<R: Read + Seek>(reader: &mut R) -> bool {
    let len = match reader.seek(SeekFrom::End(0)) {
        Ok(len) => len,
        Err(_) => return false,
    };
    let mut offset: u64 = 0;
    let mut head = [0u8; 8];
    while offset + 8 <= len {
        if reader.seek(SeekFrom::Start(offset)).is_err() || reader.read_exact(&mut head).is_err() {
            return false;
        }
        let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
        if &head[4..8] == b"moov" {
            return true;
        }
        let step = match size {
            // 长度为 0 表示「一直到文件尾」，后面不可能再有 box。
            0 => return false,
            1 => {
                let mut large = [0u8; 8];
                if reader.read_exact(&mut large).is_err() {
                    return false;
                }
                u64::from_be_bytes(large)
            }
            size => size,
        };
        if step < 8 {
            return false;
        }
        match offset.checked_add(step) {
            Some(next) => offset = next,
            None => return false,
        }
    }
    false
}

fn last_error() -> String {
    #[cfg(target_os = "macos")]
    {
        let ptr = unsafe { cs_record_last_error() };
        if ptr.is_null() {
            return "录屏失败".to_string();
        }
        let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy();
        if text.is_empty() {
            "录屏失败".to_string()
        } else {
            text.into_owned()
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        "这个平台不能录屏".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{has_moov, is_finished, Event, Job, StartSpec, Worker};
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn box_of(kind: &[u8; 4], payload: usize) -> Vec<u8> {
        let size = 8 + payload;
        let mut out = (size as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend(std::iter::repeat_n(0u8, payload));
        out
    }

    fn moov_in(bytes: Vec<u8>) -> bool {
        has_moov(&mut Cursor::new(bytes))
    }

    /// 收一条想要的回信：中间混进来的别的回信直接丢掉，测试只关系心照的那一类。
    fn await_event(
        worker: &Worker,
        within: Duration,
        want: impl Fn(&Event) -> bool,
    ) -> Option<Event> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(event) = worker.poll() {
                if want(&event) {
                    return Some(event);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    fn live_start(path: PathBuf) -> StartSpec {
        StartSpec {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            mic: false,
            system_audio: false,
            fps: 30,
            long_edge: 0,
            mic_device: String::new(),
            path,
        }
    }

    /// 排队、回信、收尾判定都在工作线程上，没开录也不该报错。
    #[test]
    fn record_jobs_are_queued_and_answered_off_the_calling_thread() {
        let worker = Worker::spawn();
        let dir = std::env::temp_dir().join(format!("clipsync-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the worker test dir");
        let path = dir.join("clip.mp4");
        let content = box_of(b"ftyp", 16);
        std::fs::write(&path, &content).expect("write the stub clip");

        worker.send(Job::Exclude);
        match await_event(&worker, Duration::from_secs(5), |event| {
            matches!(event, Event::Excluded(_))
        }) {
            Some(Event::Excluded(Ok(()))) => {}
            other => panic!("exclude answered {other:?}"),
        }
        worker.send(Job::Stop(path.clone()));
        match await_event(&worker, Duration::from_secs(5), |event| {
            matches!(event, Event::Stopped { .. })
        }) {
            Some(Event::Stopped {
                error: None,
                path: answered,
                size,
                finished,
            }) => {
                assert_eq!(answered, path);
                assert_eq!(size, content.len() as i64, "大小取自磁盘，不是字节数");
                assert!(!finished, "只有 ftyp 的半成品不该报完成");
            }
            Some(Event::Stopped {
                error: Some(error), ..
            }) => panic!("stop failed: {error}"),
            other => panic!("no stop answer, got {other:?}"),
        }
        // 字节留在盘上：写完 moov 再停一次，判定就该翻过来。
        let mut finished_clip = content;
        finished_clip.extend(box_of(b"mdat", 512));
        finished_clip.extend(box_of(b"moov", 240));
        std::fs::write(&path, &finished_clip).expect("write the finished clip");
        worker.send(Job::Stop(path.clone()));
        match await_event(&worker, Duration::from_secs(5), |event| {
            matches!(event, Event::Stopped { finished: true, .. })
        }) {
            Some(Event::Stopped { size, .. }) => assert_eq!(size, finished_clip.len() as i64),
            other => panic!("finished clip answered {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Needs screen-recording permission; run with
    /// `cargo test -p clipsync-desktop live_recording -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_live_recording_ends_with_a_playable_mp4() {
        let worker = Worker::spawn();
        let path = std::env::temp_dir().join(format!("clipsync-live-{}.mp4", std::process::id()));
        worker.send(Job::Start(live_start(path.clone())));
        match await_event(&worker, Duration::from_secs(10), |event| {
            matches!(event, Event::Started(_))
        }) {
            Some(Event::Started(Ok(()))) => {}
            Some(Event::Started(Err(error))) => panic!("start failed: {error}"),
            other => panic!("no start answer, got {other:?}"),
        }
        worker.send(Job::Exclude);
        std::thread::sleep(Duration::from_millis(1200));
        worker.send(Job::Stop(path.clone()));
        let size = match await_event(&worker, Duration::from_secs(15), |event| {
            matches!(event, Event::Stopped { .. })
        }) {
            Some(Event::Stopped {
                error, finished, ..
            }) => {
                if let Some(error) = error {
                    panic!("stop failed: {error}");
                }
                assert!(finished, "录完的文件没有 moov");
                std::fs::metadata(&path).expect("the clip is on disk").len()
            }
            other => panic!("no stop answer, got {other:?}"),
        };
        let _ = std::fs::remove_file(&path);
        assert!(size > 0, "empty mp4 in {size} bytes");
        eprintln!("live recording: {size} bytes");
    }

    #[test]
    fn a_recording_is_only_finished_when_moov_is_written() {
        let mut unfinished = box_of(b"ftyp", 16);
        unfinished.extend(box_of(b"mdat", 512));
        assert!(!moov_in(unfinished));

        let mut finished = box_of(b"ftyp", 16);
        finished.extend(box_of(b"mdat", 512));
        finished.extend(box_of(b"moov", 240));
        assert!(moov_in(finished));

        // moov 在前（faststart）也算。
        let mut faststart = box_of(b"ftyp", 16);
        faststart.extend(box_of(b"moov", 240));
        faststart.extend(box_of(b"mdat", 512));
        assert!(moov_in(faststart));
    }

    #[test]
    fn a_truncated_or_garbage_file_is_not_finished() {
        assert!(!moov_in(vec![]));
        assert!(!moov_in(b"ftyp".to_vec()));
        // 声明的长度超过实际字节：写到一半被掐断。
        let mut cut = box_of(b"ftyp", 16);
        cut.extend_from_slice(&2048u32.to_be_bytes());
        cut.extend_from_slice(b"mdat");
        assert!(!moov_in(cut));
        // 长度为 0 表示「一直到文件尾」，后面不可能再有 moov。
        let mut open_ended = box_of(b"ftyp", 16);
        open_ended.extend_from_slice(&0u32.to_be_bytes());
        open_ended.extend_from_slice(b"mdat");
        assert!(!moov_in(open_ended));
    }

    /// 判定走的是文件，不是内存里的字节。
    #[test]
    fn is_finished_reads_the_boxes_straight_off_the_disk() {
        let dir = std::env::temp_dir().join(format!("clipsync-moov-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the moov test dir");
        let path = dir.join("clip.mp4");
        let mut bytes = box_of(b"ftyp", 16);
        bytes.extend(box_of(b"mdat", 512));
        std::fs::write(&path, &bytes).expect("write the unfinished clip");
        assert!(!is_finished(&path));
        bytes.extend(box_of(b"moov", 240));
        std::fs::write(&path, &bytes).expect("write the finished clip");
        assert!(is_finished(&path));
        assert!(!is_finished(&dir.join("missing.mp4")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
