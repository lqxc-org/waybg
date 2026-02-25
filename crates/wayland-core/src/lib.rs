use gst::prelude::*;
use gstreamer as gst;
use serde::{Deserialize, Serialize};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    delegate_simple,
    dmabuf::{DmabufFeedback, DmabufHandler, DmabufState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState, SimpleGlobal},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use std::{
    collections::VecDeque,
    env,
    error::Error,
    ffi::CString,
    fs, io,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_buffer, wl_output, wl_shm, wl_surface},
};
use smithay_client_toolkit::reexports::protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1,
};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::WpViewporter,
};

pub type DynError = Box<dyn Error>;
const BLANK_VIDEO_URI: &str = "blank://";
const ARCH_CODEC_HINT: &str = "Arch Linux codec hint: install `gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav ffmpeg` with pacman.";
const WAYBG_BACKEND_ENV: &str = "WAYBG_BACKEND";
const WAYBG_SCALE_MODE_ENV: &str = "WAYBG_SCALE_MODE";
const WAYBG_DMABUF_ENV: &str = "WAYBG_DMABUF";
const BACKEND_AUTO: &str = "auto";
const BACKEND_GSTREAMER: &str = "gstreamer";
const BACKEND_LAYER_SHELL: &str = "layer-shell";
const SCALE_MODE_FIT: &str = "fit";
const SCALE_MODE_FILL: &str = "fill";
const SCALE_MODE_STRETCH: &str = "stretch";
const DMABUF_MODE_AUTO: &str = "auto";
const DMABUF_MODE_ON: &str = "on";
const DMABUF_MODE_OFF: &str = "off";
const METRICS_SCHEMA_VERSION: u32 = 1;
const METRICS_HISTORY_CAPACITY: usize = 900;
const METRICS_FLUSH_INTERVAL: Duration = Duration::from_millis(200);
const DMABUF_POOL_SIZE: usize = 2;
const MAX_IMPORTED_DMABUF_IN_FLIGHT: usize = 3;
const GST_CAPS_FEATURE_MEMORY_DMABUF: &str = "memory:DMABuf";
const GST_MEMORY_TYPE_DMABUF: &str = "dmabuf";
const GST_VIDEO_MAX_PLANES: usize = 4;

const DMA_HEAP_DEVICE_CANDIDATES: &[&str] = &[
    "/dev/dma_heap/system",
    "/dev/dma_heap/linux,cma",
    "/dev/dma_heap/reserved",
];

const fn fourcc_code(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

// Wayland ARGB8888 uses little-endian BGRA byte order, matching appsink BGRA frames.
const DRM_FORMAT_ARGB8888: u32 = fourcc_code(b'A', b'R', b'2', b'4');
const DRM_FORMAT_XRGB8888: u32 = fourcc_code(b'X', b'R', b'2', b'4');
const DRM_FORMAT_MOD_LINEAR: u64 = 0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackMetricsSnapshot {
    pub schema_version: u32,
    pub backend: String,
    pub input: String,
    pub output: Option<String>,
    pub sample_count: usize,
    pub avg_fps: f64,
    pub low95_fps: f64,
    pub low99_fps: f64,
    pub min_fps: f64,
    pub max_fps: f64,
    pub last_fps: f64,
    pub updated_unix_ms: u64,
    pub recent_fps: Vec<f64>,
    pub hardware_decoders: Vec<String>,
    pub notes: Option<String>,
}

struct MetricsRecorder {
    path: PathBuf,
    backend: String,
    input: String,
    output: Option<String>,
    hardware_decoders: Vec<String>,
    samples: VecDeque<f64>,
    sample_count: usize,
    last_fps: f64,
    previous_frame_instant: Option<Instant>,
    last_flush_instant: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackBackend {
    GstreamerWindow,
    LayerShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScaleMode {
    Fit,
    Fill,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmabufMode {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone)]
struct VideoFrame {
    width: u32,
    height: u32,
    stride: usize,
    pixels: Vec<u8>,
}

type SharedFrame = Arc<VideoFrame>;

#[derive(Clone)]
enum FramePayload {
    Cpu(SharedFrame),
    Dmabuf(Arc<DmabufVideoFrame>),
}

struct DmabufVideoFrame {
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    planes: Vec<DmabufPlane>,
    sample: gst::Sample,
}

struct DmabufPlane {
    fd: OwnedFd,
    offset: u32,
    stride: u32,
}

struct ImportedDmabufFrame {
    wl_buffer: wl_buffer::WlBuffer,
    _frame: Arc<DmabufVideoFrame>,
}

#[repr(C)]
struct DmaHeapAllocationData {
    len: u64,
    fd: u32,
    fd_flags: u32,
    heap_flags: u64,
}

#[repr(C)]
struct GstVideoMetaPrefix {
    _meta: gst::ffi::GstMeta,
    _buffer: *mut gst::ffi::GstBuffer,
    _flags: libc::c_int,
    _format: libc::c_int,
    _id: libc::c_int,
    _width: u32,
    _height: u32,
    n_planes: u32,
    offset: [usize; GST_VIDEO_MAX_PLANES],
    stride: [i32; GST_VIDEO_MAX_PLANES],
}

struct DmaHeapBuffer {
    fd: OwnedFd,
    ptr: *mut u8,
    len: usize,
}

struct DmabufSurfaceBuffer {
    wl_buffer: wl_buffer::WlBuffer,
    memory: DmaHeapBuffer,
    released: bool,
}

struct WallpaperSurface {
    layer: LayerSurface,
    viewport: Option<WpViewport>,
    width: u32,
    height: u32,
    scale_factor: i32,
    transform: wl_output::Transform,
    first_configure: bool,
    buffer_width: u32,
    buffer_height: u32,
    buffer: Option<Buffer>,
    dmabuf_buffers: Vec<DmabufSurfaceBuffer>,
    imported_dmabuf_frames: Vec<ImportedDmabufFrame>,
}

struct LayerWallpaperState {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    output_state: OutputState,
    shm_state: Shm,
    dmabuf_state: DmabufState,
    dmabuf_enabled: bool,
    dmabuf_required: bool,
    dma_heap_fd: Option<OwnedFd>,
    wp_viewporter: Option<SimpleGlobal<WpViewporter, 1>>,
    layer_shell_state: LayerShell,
    pool: SlotPool,
    surfaces: Vec<WallpaperSurface>,
    frame_store: Arc<Mutex<Option<FramePayload>>>,
    scale_mode: ScaleMode,
    stop: Arc<AtomicBool>,
    exit: bool,
    fatal_error: Option<String>,
}

impl DmaHeapBuffer {
    fn allocate(heap_fd: &OwnedFd, len: usize) -> Result<Self, io::Error> {
        let aligned_len = align_up(len.max(1), 4096);
        let fd = dma_heap_alloc_fd(heap_fd, aligned_len)?;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                aligned_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            fd,
            ptr: ptr.cast(),
            len: aligned_len,
        })
    }

    fn canvas_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for DmaHeapBuffer {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.len > 0 {
            let _ = unsafe { libc::munmap(self.ptr.cast(), self.len) };
        }
    }
}

impl Drop for DmabufSurfaceBuffer {
    fn drop(&mut self) {
        self.wl_buffer.destroy();
    }
}

impl Drop for ImportedDmabufFrame {
    fn drop(&mut self) {
        self.wl_buffer.destroy();
    }
}

impl FramePayload {
    fn dimensions(&self) -> (u32, u32) {
        match self {
            FramePayload::Cpu(frame) => (frame.width.max(1), frame.height.max(1)),
            FramePayload::Dmabuf(frame) => (frame.width.max(1), frame.height.max(1)),
        }
    }

    fn cpu_frame(&self) -> Option<&VideoFrame> {
        match self {
            FramePayload::Cpu(frame) => Some(frame.as_ref()),
            FramePayload::Dmabuf(_) => None,
        }
    }

    fn dmabuf_frame(&self) -> Option<&Arc<DmabufVideoFrame>> {
        match self {
            FramePayload::Cpu(_) => None,
            FramePayload::Dmabuf(frame) => Some(frame),
        }
    }
}

impl MetricsRecorder {
    fn new(
        path: PathBuf,
        backend: &str,
        input: &str,
        output: Option<&str>,
        hardware_decoders: Vec<String>,
    ) -> Self {
        Self {
            path,
            backend: backend.to_string(),
            input: input.to_string(),
            output: output.map(ToOwned::to_owned),
            hardware_decoders,
            samples: VecDeque::with_capacity(METRICS_HISTORY_CAPACITY),
            sample_count: 0,
            last_fps: 0.0,
            previous_frame_instant: None,
            last_flush_instant: Instant::now(),
        }
    }

    fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(previous) = self.previous_frame_instant.replace(now) {
            let delta = now.saturating_duration_since(previous).as_secs_f64();
            if delta > 0.0 {
                let fps = (1.0 / delta).clamp(0.0, 1000.0);
                self.last_fps = fps;
                self.sample_count += 1;
                if self.samples.len() == METRICS_HISTORY_CAPACITY {
                    self.samples.pop_front();
                }
                self.samples.push_back(fps);
            }
        }
    }

    fn flush_if_due(&mut self, force: bool, notes: Option<&str>) -> Result<(), io::Error> {
        if !force && self.last_flush_instant.elapsed() < METRICS_FLUSH_INTERVAL {
            return Ok(());
        }
        let snapshot = self.snapshot(notes);
        write_metrics_snapshot(&self.path, &snapshot)?;
        self.last_flush_instant = Instant::now();
        Ok(())
    }

    fn snapshot(&self, notes: Option<&str>) -> PlaybackMetricsSnapshot {
        let recent_fps = self.samples.iter().copied().collect::<Vec<_>>();
        let avg_fps = mean_fps(&recent_fps);
        let low95_fps = percentile_low_fps(&recent_fps, 0.95);
        let low99_fps = percentile_low_fps(&recent_fps, 0.99);
        let min_fps = recent_fps.iter().copied().reduce(f64::min).unwrap_or(0.0);
        let max_fps = recent_fps.iter().copied().reduce(f64::max).unwrap_or(0.0);
        PlaybackMetricsSnapshot {
            schema_version: METRICS_SCHEMA_VERSION,
            backend: self.backend.clone(),
            input: self.input.clone(),
            output: self.output.clone(),
            sample_count: self.sample_count,
            avg_fps,
            low95_fps,
            low99_fps,
            min_fps,
            max_fps,
            last_fps: self.last_fps,
            updated_unix_ms: unix_timestamp_ms(),
            recent_fps,
            hardware_decoders: self.hardware_decoders.clone(),
            notes: notes.map(ToOwned::to_owned),
        }
    }
}

fn write_metrics_snapshot(
    path: &Path,
    snapshot: &PlaybackMetricsSnapshot,
) -> Result<(), io::Error> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_string_pretty(snapshot)
        .map_err(|error| io::Error::other(format!("failed to encode metrics snapshot: {error}")))?;
    fs::write(path, encoded)?;
    Ok(())
}

fn write_placeholder_metrics(
    metrics_file: Option<&Path>,
    backend: &str,
    input: &str,
    output: Option<&str>,
    hardware_decoders: &[String],
    notes: Option<&str>,
) {
    let Some(path) = metrics_file else {
        return;
    };
    let snapshot = PlaybackMetricsSnapshot {
        schema_version: METRICS_SCHEMA_VERSION,
        backend: backend.to_string(),
        input: input.to_string(),
        output: output.map(ToOwned::to_owned),
        sample_count: 0,
        avg_fps: 0.0,
        low95_fps: 0.0,
        low99_fps: 0.0,
        min_fps: 0.0,
        max_fps: 0.0,
        last_fps: 0.0,
        updated_unix_ms: unix_timestamp_ms(),
        recent_fps: Vec::new(),
        hardware_decoders: hardware_decoders.to_vec(),
        notes: notes.map(ToOwned::to_owned),
    };
    if let Err(error) = write_metrics_snapshot(path, &snapshot) {
        eprintln!(
            "warning: failed to write playback metrics to '{}': {error}",
            path.display()
        );
    }
}

fn mean_fps(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples.iter().sum::<f64>();
    sum / samples.len() as f64
}

fn percentile_low_fps(samples: &[f64], keep_ratio: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let tail_ratio = (1.0 - keep_ratio).clamp(0.0, 1.0);
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let index = ((sorted.len() - 1) as f64 * tail_ratio).round() as usize;
    sorted[index]
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn play_video(
    input: &str,
    loop_playback: bool,
    output: Option<&str>,
    mute: bool,
    metrics_file: Option<&Path>,
) -> Result<(), DynError> {
    match resolve_playback_backend()? {
        PlaybackBackend::LayerShell => {
            play_video_layer_shell(input, loop_playback, output, mute, metrics_file)
        }
        PlaybackBackend::GstreamerWindow => {
            play_video_gstreamer_window(input, loop_playback, output, mute, metrics_file)
        }
    }
}

fn play_video_layer_shell(
    input: &str,
    loop_playback: bool,
    output: Option<&str>,
    mute: bool,
    metrics_file: Option<&Path>,
) -> Result<(), DynError> {
    let frame_store = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let requested_output = output.map(ToOwned::to_owned);
    let scale_mode = resolve_scale_mode()?;
    let dmabuf_mode = resolve_dmabuf_mode()?;

    let renderer_frame_store = Arc::clone(&frame_store);
    let renderer_stop = Arc::clone(&stop);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let renderer = thread::spawn(move || {
        run_layer_renderer(
            renderer_frame_store,
            renderer_stop,
            requested_output,
            scale_mode,
            dmabuf_mode,
            ready_tx,
        )
    });

    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            stop.store(true, Ordering::Relaxed);
            let _ = join_renderer_thread(renderer);
            return Err(io::Error::other(error).into());
        }
        Err(error) => {
            stop.store(true, Ordering::Relaxed);
            let _ = join_renderer_thread(renderer);
            return Err(io::Error::other(format!(
                "layer renderer failed to report startup status: {error}"
            ))
            .into());
        }
    }

    if is_blank_source(input) {
        write_placeholder_metrics(
            metrics_file,
            BACKEND_LAYER_SHELL,
            input,
            output,
            &[],
            Some("blank source does not emit FPS samples"),
        );
        println!(
            "Playing blank layer-shell background (loop={loop_playback}, output={}, scale-mode={})",
            output.unwrap_or("<all>"),
            scale_mode_name(scale_mode)
        );
        if loop_playback {
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(200));
            }
        } else {
            thread::sleep(Duration::from_millis(400));
            stop.store(true, Ordering::Relaxed);
        }
        return join_renderer_thread(renderer);
    }

    let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());

    gst::init()
        .map_err(|error| io::Error::other(format!("failed to initialize GStreamer: {error}")))?;
    let hardware_decoders = configure_hardware_decoder_preference();
    warn_about_codec_runtime();

    let uri = to_uri(input)?;
    let playbin = gst::ElementFactory::make("playbin")
        .name("player")
        .build()
        .map_err(|_| io::Error::other("GStreamer element 'playbin' is unavailable"))?;

    let appsink = gst::ElementFactory::make("appsink")
        .name("frame_sink")
        .build()
        .map_err(|_| io::Error::other("GStreamer element 'appsink' is unavailable"))?;

    let caps = build_appsink_caps(dmabuf_mode);
    if appsink.find_property("caps").is_some() {
        appsink.set_property("caps", &caps);
    }
    if appsink.find_property("emit-signals").is_some() {
        appsink.set_property("emit-signals", false);
    }
    if appsink.find_property("sync").is_some() {
        appsink.set_property("sync", true);
    }
    if appsink.find_property("max-buffers").is_some() {
        appsink.set_property("max-buffers", 8u32);
    }
    if appsink.find_property("drop").is_some() {
        appsink.set_property("drop", false);
    }

    playbin.set_property("video-sink", &appsink);
    playbin.set_property("uri", &uri);
    playbin.set_property("mute", mute);

    let bus = playbin
        .bus()
        .ok_or_else(|| io::Error::other("failed to retrieve GStreamer bus"))?;

    playbin.set_state(gst::State::Playing).map_err(|error| {
        io::Error::other(format!("failed to set pipeline to Playing: {error:?}"))
    })?;

    let mut metrics_recorder = metrics_file.map(|path| {
        MetricsRecorder::new(
            path.to_path_buf(),
            BACKEND_LAYER_SHELL,
            input,
            output,
            hardware_decoders.clone(),
        )
    });

    println!(
        "Playing layer-shell background on Wayland display '{wayland_display}': {uri} (loop={loop_playback}, output={}, mute={mute}, scale-mode={})",
        output.unwrap_or("<all>"),
        scale_mode_name(scale_mode)
    );

    let mut playback_error: Option<io::Error> = None;
    while !stop.load(Ordering::Relaxed) {
        if let Some(sample) = try_pull_sample(&appsink) {
            match sample_to_frame_payload(sample, !matches!(dmabuf_mode, DmabufMode::Off)) {
                Ok(frame_payload) => {
                    if let Ok(mut slot) = frame_store.lock() {
                        *slot = Some(frame_payload);
                    }
                    if let Some(recorder) = metrics_recorder.as_mut() {
                        recorder.record_frame();
                        if let Err(error) = recorder.flush_if_due(false, None) {
                            eprintln!("warning: failed to flush playback metrics: {error}");
                        }
                    }
                }
                Err(error) => {
                    eprintln!("warning: failed to decode sample frame: {error}");
                }
            }
        }

        let mut reached_eos = false;
        while let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(0)) {
            use gst::MessageView;

            match message.view() {
                MessageView::Eos(..) => {
                    reached_eos = true;
                }
                MessageView::Error(error) => {
                    let source = error
                        .src()
                        .map(|src| src.path_string())
                        .unwrap_or_else(|| "unknown".into());
                    playback_error = Some(io::Error::other(format!(
                        "GStreamer error from {source}: {} ({:?})",
                        error.error(),
                        error.debug()
                    )));
                    break;
                }
                _ => {}
            }
        }

        if let Some(error) = playback_error.take() {
            let error_message = error.to_string();
            stop.store(true, Ordering::Relaxed);
            let _ = playbin.set_state(gst::State::Null);
            let _ = join_renderer_thread(renderer);
            if let Some(recorder) = metrics_recorder.as_mut()
                && let Err(metrics_error) = recorder.flush_if_due(true, Some(&error_message))
            {
                eprintln!("warning: failed to flush playback metrics: {metrics_error}");
            }
            return Err(error.into());
        }

        if reached_eos {
            if loop_playback {
                playbin
                    .seek_simple(
                        gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                        gst::ClockTime::ZERO,
                    )
                    .map_err(|error| {
                        io::Error::other(format!(
                            "failed to seek to start for looped playback: {error}"
                        ))
                    })?;
            } else {
                stop.store(true, Ordering::Relaxed);
                break;
            }
        }

        thread::sleep(Duration::from_millis(8));
    }

    playbin
        .set_state(gst::State::Null)
        .map_err(|error| io::Error::other(format!("failed to set pipeline to Null: {error:?}")))?;

    if let Some(recorder) = metrics_recorder.as_mut()
        && let Err(error) = recorder.flush_if_due(true, Some("playback stopped"))
    {
        eprintln!("warning: failed to flush playback metrics: {error}");
    }

    stop.store(true, Ordering::Relaxed);
    join_renderer_thread(renderer)
}

fn join_renderer_thread(
    renderer: thread::JoinHandle<Result<(), io::Error>>,
) -> Result<(), DynError> {
    match renderer.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => Err(io::Error::other("layer renderer thread panicked").into()),
    }
}

fn run_layer_renderer(
    frame_store: Arc<Mutex<Option<FramePayload>>>,
    stop: Arc<AtomicBool>,
    requested_output_name: Option<String>,
    scale_mode: ScaleMode,
    dmabuf_mode: DmabufMode,
    ready_tx: mpsc::Sender<Result<(), String>>,
) -> Result<(), io::Error> {
    let conn = Connection::connect_to_env().map_err(|error| {
        io::Error::other(format!("failed to connect to Wayland server: {error}"))
    })?;

    let (globals, mut event_queue) = registry_queue_init(&conn).map_err(|error| {
        io::Error::other(format!("failed to initialize Wayland registry: {error}"))
    })?;
    let qh = event_queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh)
        .map_err(|error| io::Error::other(format!("wl_compositor is unavailable: {error}")))?;
    let layer_shell_state = LayerShell::bind(&globals, &qh)
        .map_err(|error| io::Error::other(format!("layer shell is unavailable: {error}")))?;
    let shm_state = Shm::bind(&globals, &qh)
        .map_err(|error| io::Error::other(format!("wl_shm is unavailable: {error}")))?;
    let dmabuf_state = DmabufState::new(&globals, &qh);
    let wp_viewporter = SimpleGlobal::<WpViewporter, 1>::bind(&globals, &qh).ok();
    let compositor_scaling_enabled =
        wp_viewporter.is_some() && !matches!(scale_mode, ScaleMode::Fit);

    let (dmabuf_enabled, dmabuf_required, dma_heap_fd) = match dmabuf_mode {
        DmabufMode::Off => (false, false, None),
        DmabufMode::Auto | DmabufMode::On => {
            let protocol_supported = dmabuf_state.version().is_some();
            if !protocol_supported {
                if matches!(dmabuf_mode, DmabufMode::On) {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "WAYBG_DMABUF=on, but compositor does not expose zwp_linux_dmabuf_v1",
                    ));
                }
                println!("waybg renderer: compositor does not expose dmabuf, using wl_shm.");
                (false, false, None)
            } else {
                match open_dma_heap_device() {
                    Ok(fd) => (true, matches!(dmabuf_mode, DmabufMode::On), Some(fd)),
                    Err(error) => {
                        if matches!(dmabuf_mode, DmabufMode::On) {
                            return Err(io::Error::other(format!(
                                "WAYBG_DMABUF=on, but opening dma_heap failed: {error}"
                            )));
                        }
                        eprintln!(
                            "waybg renderer: dma_heap unavailable ({error}), falling back to wl_shm."
                        );
                        (false, false, None)
                    }
                }
            }
        }
    };

    let pool = SlotPool::new(4, &shm_state).map_err(|error| {
        io::Error::other(format!("failed to allocate shared memory pool: {error}"))
    })?;

    let mut state = LayerWallpaperState {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        compositor_state,
        shm_state,
        dmabuf_state,
        dmabuf_enabled,
        dmabuf_required,
        dma_heap_fd,
        wp_viewporter,
        layer_shell_state,
        pool,
        surfaces: Vec::new(),
        frame_store,
        scale_mode,
        stop,
        exit: false,
        fatal_error: None,
    };

    event_queue
        .roundtrip(&mut state)
        .map_err(|error| io::Error::other(format!("failed to collect output metadata: {error}")))?;

    let targets = select_target_outputs(&state.output_state, requested_output_name.as_deref())?;
    if targets.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no Wayland outputs were detected",
        ));
    }

    if compositor_scaling_enabled {
        println!(
            "waybg renderer: compositor scaling enabled via wp_viewporter (scale mode: {})",
            scale_mode_name(scale_mode)
        );
    } else if !matches!(scale_mode, ScaleMode::Fit) {
        eprintln!(
            "waybg renderer: wp_viewporter unavailable, falling back to CPU scaling (scale mode: {})",
            scale_mode_name(scale_mode)
        );
    }

    if state.dmabuf_enabled {
        println!("waybg renderer: dmabuf path enabled.");
    } else if matches!(dmabuf_mode, DmabufMode::On) {
        return Err(io::Error::other(
            "WAYBG_DMABUF=on requested, but dmabuf path is not available",
        ));
    } else {
        println!("waybg renderer: using wl_shm path.");
    }

    for (wl_output, _name) in targets {
        let wl_surface = state.compositor_state.create_surface(&qh);
        let layer = state.layer_shell_state.create_layer_surface(
            &qh,
            wl_surface,
            Layer::Background,
            Some("waybg"),
            Some(&wl_output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_exclusive_zone(0);
        layer.set_size(0, 0);
        layer.commit();

        let viewport = if compositor_scaling_enabled {
            state
                .wp_viewporter
                .as_ref()
                .and_then(|global| global.get().ok())
                .map(|viewporter| viewporter.get_viewport(layer.wl_surface(), &qh, ()))
        } else {
            None
        };

        state.surfaces.push(WallpaperSurface {
            layer,
            viewport,
            width: 1,
            height: 1,
            scale_factor: 1,
            transform: wl_output::Transform::Normal,
            first_configure: true,
            buffer_width: 0,
            buffer_height: 0,
            buffer: None,
            dmabuf_buffers: Vec::new(),
            imported_dmabuf_frames: Vec::new(),
        });
    }

    let _ = ready_tx.send(Ok(()));

    loop {
        if state.stop.load(Ordering::Relaxed) || state.exit {
            break;
        }

        event_queue
            .blocking_dispatch(&mut state)
            .map_err(|error| io::Error::other(format!("Wayland dispatch failed: {error}")))?;

        if let Some(error) = state.fatal_error.take() {
            return Err(io::Error::other(error));
        }
    }

    Ok(())
}

fn select_target_outputs(
    output_state: &OutputState,
    requested_output_name: Option<&str>,
) -> Result<Vec<(wl_output::WlOutput, Option<String>)>, io::Error> {
    let mut outputs = Vec::new();
    for output in output_state.outputs() {
        let name = output_state.info(&output).and_then(|info| info.name);
        outputs.push((output, name));
    }

    if outputs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no outputs advertised by the compositor",
        ));
    }

    let Some(requested_name) = requested_output_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return Ok(outputs);
    };

    if let Some(found) = outputs
        .iter()
        .find(|(_, name)| name.as_deref() == Some(requested_name))
    {
        return Ok(vec![(found.0.clone(), found.1.clone())]);
    }

    let available = outputs
        .iter()
        .filter_map(|(_, name)| name.clone())
        .collect::<Vec<_>>();

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "requested output '{requested_name}' was not found (available outputs: {})",
            if available.is_empty() {
                "<none named>".to_string()
            } else {
                available.join(", ")
            }
        ),
    ))
}

impl LayerWallpaperState {
    fn draw_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        surface_index: usize,
    ) -> Result<(), io::Error> {
        let current_frame = self
            .frame_store
            .lock()
            .map_err(|_| io::Error::other("frame store lock was poisoned"))?
            .clone();
        let frame_payload = current_frame.as_ref();
        let frame_cpu = frame_payload.and_then(FramePayload::cpu_frame);
        let frame_dmabuf = frame_payload.and_then(FramePayload::dmabuf_frame);
        let cpu_fallback_from_dmabuf = if frame_cpu.is_none() {
            frame_dmabuf.and_then(|dmabuf_frame| dmabuf_frame_to_video_frame(dmabuf_frame.as_ref()))
        } else {
            None
        };
        let effective_cpu_frame = frame_cpu.or(cpu_fallback_from_dmabuf.as_ref());

        let surface = self
            .surfaces
            .get(surface_index)
            .ok_or_else(|| io::Error::other("surface index out of range"))?;
        let logical_width = surface.width.max(1);
        let logical_height = surface.height.max(1);
        let use_compositor_scaling =
            surface.viewport.is_some() && !matches!(self.scale_mode, ScaleMode::Fit);
        let surface_scale_factor = surface.scale_factor.max(1);
        let surface_transform = surface.transform;

        let (buffer_width, buffer_height, buffer_scale) = if use_compositor_scaling {
            let (source_width, source_height) = frame_payload
                .map(FramePayload::dimensions)
                .unwrap_or((1, 1));
            (source_width, source_height, 1i32)
        } else {
            let buffer_scale = surface_scale_factor as u32;
            let mut buffer_width = logical_width.saturating_mul(buffer_scale);
            let mut buffer_height = logical_height.saturating_mul(buffer_scale);
            if transform_swaps_axes(surface_transform) {
                std::mem::swap(&mut buffer_width, &mut buffer_height);
            }
            (buffer_width, buffer_height, surface_scale_factor)
        };

        if self.dmabuf_enabled {
            match self.draw_surface_dmabuf(
                qh,
                surface_index,
                frame_payload,
                logical_width,
                logical_height,
                buffer_width,
                buffer_height,
                buffer_scale,
                use_compositor_scaling,
            ) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(error) => {
                    if self.dmabuf_required {
                        return Err(error);
                    }
                    eprintln!(
                        "waybg renderer: dmabuf path failed, falling back to wl_shm: {error}"
                    );
                    self.disable_dmabuf();
                }
            }
        }

        self.draw_surface_shm(
            qh,
            surface_index,
            effective_cpu_frame,
            logical_width,
            logical_height,
            buffer_width,
            buffer_height,
            buffer_scale,
            use_compositor_scaling,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_surface_shm(
        &mut self,
        qh: &QueueHandle<Self>,
        surface_index: usize,
        frame: Option<&VideoFrame>,
        logical_width: u32,
        logical_height: u32,
        buffer_width: u32,
        buffer_height: u32,
        buffer_scale: i32,
        use_compositor_scaling: bool,
    ) -> Result<(), io::Error> {
        let stride = buffer_width as i32 * 4;
        let (pool, surfaces) = (&mut self.pool, &mut self.surfaces);
        let surface = surfaces
            .get_mut(surface_index)
            .ok_or_else(|| io::Error::other("surface index out of range"))?;

        if surface.buffer.is_none()
            || surface.buffer_width != buffer_width
            || surface.buffer_height != buffer_height
        {
            let (buffer, _) = pool
                .create_buffer(
                    buffer_width as i32,
                    buffer_height as i32,
                    stride,
                    wl_shm::Format::Argb8888,
                )
                .map_err(|error| {
                    io::Error::other(format!("failed to create shm buffer: {error}"))
                })?;
            surface.buffer = Some(buffer);
            surface.buffer_width = buffer_width;
            surface.buffer_height = buffer_height;
        }

        let buffer = surface
            .buffer
            .as_mut()
            .ok_or_else(|| io::Error::other("missing surface buffer"))?;

        let canvas = match pool.canvas(buffer) {
            Some(canvas) => canvas,
            None => {
                let (next_buffer, canvas) = pool
                    .create_buffer(
                        buffer_width as i32,
                        buffer_height as i32,
                        stride,
                        wl_shm::Format::Argb8888,
                    )
                    .map_err(|error| {
                        io::Error::other(format!("failed to create fallback shm buffer: {error}"))
                    })?;
                *buffer = next_buffer;
                surface.buffer_width = buffer_width;
                surface.buffer_height = buffer_height;
                canvas
            }
        };

        if use_compositor_scaling {
            if let Some(frame) = frame {
                copy_frame_to_canvas(frame, canvas, buffer_width, buffer_height);
            } else {
                fill_black(canvas);
            }
            if let Some(viewport) = surface.viewport.as_ref() {
                viewport.set_destination(logical_width as i32, logical_height as i32);
                configure_viewport_source(
                    viewport,
                    frame.map(|entry| (entry.width, entry.height)),
                    logical_width,
                    logical_height,
                    self.scale_mode,
                );
            }
        } else {
            fill_canvas_for_surface(canvas, frame, buffer_width, buffer_height, self.scale_mode);
        }

        let wl_surface = surface.layer.wl_surface();
        wl_surface.set_buffer_scale(buffer_scale);
        wl_surface.set_buffer_transform(surface.transform);
        wl_surface.damage_buffer(0, 0, buffer_width as i32, buffer_height as i32);
        wl_surface.frame(qh, wl_surface.clone());
        buffer
            .attach_to(wl_surface)
            .map_err(|error| io::Error::other(format!("failed to attach shm buffer: {error}")))?;
        surface.layer.commit();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_surface_dmabuf(
        &mut self,
        qh: &QueueHandle<Self>,
        surface_index: usize,
        frame_payload: Option<&FramePayload>,
        logical_width: u32,
        logical_height: u32,
        buffer_width: u32,
        buffer_height: u32,
        buffer_scale: i32,
        use_compositor_scaling: bool,
    ) -> Result<bool, io::Error> {
        if !self.dmabuf_enabled {
            return Ok(false);
        }
        if use_compositor_scaling
            && let Some(dmabuf_frame) = frame_payload.and_then(FramePayload::dmabuf_frame)
        {
            self.draw_surface_dmabuf_imported(
                qh,
                surface_index,
                Arc::clone(dmabuf_frame),
                logical_width,
                logical_height,
                buffer_width,
                buffer_height,
                buffer_scale,
            )?;
            return Ok(true);
        }
        self.ensure_dmabuf_buffers(qh, surface_index, buffer_width, buffer_height)?;

        let surface = self
            .surfaces
            .get_mut(surface_index)
            .ok_or_else(|| io::Error::other("surface index out of range"))?;

        let Some(buffer_index) = surface
            .dmabuf_buffers
            .iter()
            .position(|entry| entry.released)
        else {
            let wl_surface = surface.layer.wl_surface();
            wl_surface.frame(qh, wl_surface.clone());
            surface.layer.commit();
            return Ok(true);
        };

        let surface_buffer = surface
            .dmabuf_buffers
            .get_mut(buffer_index)
            .ok_or_else(|| io::Error::other("dmabuf index out of range"))?;
        let canvas = surface_buffer.memory.canvas_mut();
        let frame = frame_payload.and_then(FramePayload::cpu_frame);
        if use_compositor_scaling {
            if let Some(frame) = frame {
                copy_frame_to_canvas(frame, canvas, buffer_width, buffer_height);
            } else {
                fill_black(canvas);
            }
            if let Some(viewport) = surface.viewport.as_ref() {
                viewport.set_destination(logical_width as i32, logical_height as i32);
                configure_viewport_source(
                    viewport,
                    frame.map(|entry| (entry.width, entry.height)),
                    logical_width,
                    logical_height,
                    self.scale_mode,
                );
            }
        } else {
            fill_canvas_for_surface(canvas, frame, buffer_width, buffer_height, self.scale_mode);
        }

        let wl_surface = surface.layer.wl_surface();
        wl_surface.set_buffer_scale(buffer_scale);
        wl_surface.set_buffer_transform(surface.transform);
        wl_surface.damage_buffer(0, 0, buffer_width as i32, buffer_height as i32);
        wl_surface.frame(qh, wl_surface.clone());
        wl_surface.attach(Some(&surface_buffer.wl_buffer), 0, 0);
        surface_buffer.released = false;
        surface.layer.commit();
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_surface_dmabuf_imported(
        &mut self,
        qh: &QueueHandle<Self>,
        surface_index: usize,
        frame: Arc<DmabufVideoFrame>,
        logical_width: u32,
        logical_height: u32,
        buffer_width: u32,
        buffer_height: u32,
        buffer_scale: i32,
    ) -> Result<(), io::Error> {
        if self.surfaces.get(surface_index).is_some_and(|surface| {
            surface.imported_dmabuf_frames.len() >= MAX_IMPORTED_DMABUF_IN_FLIGHT
        }) {
            let surface = self
                .surfaces
                .get_mut(surface_index)
                .ok_or_else(|| io::Error::other("surface index out of range"))?;
            let wl_surface = surface.layer.wl_surface();
            wl_surface.frame(qh, wl_surface.clone());
            surface.layer.commit();
            return Ok(());
        }
        let wl_buffer = self.create_dmabuf_imported_buffer(qh, frame.as_ref())?;
        let surface = self
            .surfaces
            .get_mut(surface_index)
            .ok_or_else(|| io::Error::other("surface index out of range"))?;

        if let Some(viewport) = surface.viewport.as_ref() {
            viewport.set_destination(logical_width as i32, logical_height as i32);
            configure_viewport_source(
                viewport,
                Some((frame.width, frame.height)),
                logical_width,
                logical_height,
                self.scale_mode,
            );
        }

        let wl_surface = surface.layer.wl_surface();
        wl_surface.set_buffer_scale(buffer_scale);
        wl_surface.set_buffer_transform(surface.transform);
        wl_surface.damage_buffer(0, 0, buffer_width as i32, buffer_height as i32);
        wl_surface.frame(qh, wl_surface.clone());
        wl_surface.attach(Some(&wl_buffer), 0, 0);
        surface.imported_dmabuf_frames.push(ImportedDmabufFrame {
            wl_buffer,
            _frame: frame,
        });
        surface.layer.commit();
        Ok(())
    }

    fn ensure_dmabuf_buffers(
        &mut self,
        qh: &QueueHandle<Self>,
        surface_index: usize,
        buffer_width: u32,
        buffer_height: u32,
    ) -> Result<(), io::Error> {
        let needs_recreate = match self.surfaces.get(surface_index) {
            Some(surface) => {
                surface.dmabuf_buffers.is_empty()
                    || surface.buffer_width != buffer_width
                    || surface.buffer_height != buffer_height
            }
            None => true,
        };
        if !needs_recreate {
            return Ok(());
        }

        let heap_fd = self
            .dma_heap_fd
            .as_ref()
            .ok_or_else(|| io::Error::other("dma_heap fd is unavailable"))?;
        let stride = buffer_width.saturating_mul(4);
        let mut dmabuf_buffers = Vec::with_capacity(DMABUF_POOL_SIZE);
        for _ in 0..DMABUF_POOL_SIZE {
            dmabuf_buffers.push(self.create_dmabuf_surface_buffer(
                qh,
                heap_fd,
                buffer_width,
                buffer_height,
                stride,
            )?);
        }

        let surface = self
            .surfaces
            .get_mut(surface_index)
            .ok_or_else(|| io::Error::other("surface index out of range"))?;
        surface.buffer = None;
        surface.dmabuf_buffers = dmabuf_buffers;
        surface.buffer_width = buffer_width;
        surface.buffer_height = buffer_height;
        Ok(())
    }

    fn create_dmabuf_surface_buffer(
        &self,
        qh: &QueueHandle<Self>,
        heap_fd: &OwnedFd,
        width: u32,
        height: u32,
        stride: u32,
    ) -> Result<DmabufSurfaceBuffer, io::Error> {
        let len = (stride as usize).saturating_mul(height as usize);
        let memory = DmaHeapBuffer::allocate(heap_fd, len)?;
        let params = self
            .dmabuf_state
            .create_params(qh)
            .map_err(|error| io::Error::other(format!("dmabuf params unavailable: {error}")))?;
        params.add(memory.fd.as_fd(), 0, 0, stride, DRM_FORMAT_MOD_LINEAR);
        let (wl_buffer, params_proxy) = params.create_immed(
            width as i32,
            height as i32,
            DRM_FORMAT_ARGB8888,
            zwp_linux_buffer_params_v1::Flags::empty(),
            qh,
        );
        params_proxy.destroy();
        Ok(DmabufSurfaceBuffer {
            wl_buffer,
            memory,
            released: true,
        })
    }

    fn create_dmabuf_imported_buffer(
        &self,
        qh: &QueueHandle<Self>,
        frame: &DmabufVideoFrame,
    ) -> Result<wl_buffer::WlBuffer, io::Error> {
        if frame.planes.is_empty() {
            return Err(io::Error::other("dmabuf frame has no planes"));
        }
        let params = self
            .dmabuf_state
            .create_params(qh)
            .map_err(|error| io::Error::other(format!("dmabuf params unavailable: {error}")))?;
        let mut imported_fds = Vec::with_capacity(frame.planes.len());
        for plane in &frame.planes {
            imported_fds.push(dup_fd_cloexec(plane.fd.as_raw_fd())?);
        }
        for (plane_index, (plane, imported_fd)) in
            frame.planes.iter().zip(imported_fds.iter()).enumerate()
        {
            params.add(
                imported_fd.as_fd(),
                plane_index as u32,
                plane.offset,
                plane.stride,
                frame.modifier,
            );
        }
        let (wl_buffer, params_proxy) = params.create_immed(
            frame.width as i32,
            frame.height as i32,
            frame.format,
            zwp_linux_buffer_params_v1::Flags::empty(),
            qh,
        );
        params_proxy.destroy();
        Ok(wl_buffer)
    }

    fn disable_dmabuf(&mut self) {
        self.dmabuf_enabled = false;
        for surface in &mut self.surfaces {
            surface.dmabuf_buffers.clear();
            surface.imported_dmabuf_frames.clear();
        }
    }
}

fn fill_canvas_for_surface(
    canvas: &mut [u8],
    frame: Option<&VideoFrame>,
    dst_width: u32,
    dst_height: u32,
    scale_mode: ScaleMode,
) {
    if let Some(frame) = frame {
        blit_scaled_bgra(frame, canvas, dst_width, dst_height, scale_mode);
    } else {
        fill_black(canvas);
    }
}

fn copy_frame_to_canvas(frame: &VideoFrame, canvas: &mut [u8], dst_width: u32, dst_height: u32) {
    if frame.width != dst_width || frame.height != dst_height {
        blit_scaled_bgra(frame, canvas, dst_width, dst_height, ScaleMode::Stretch);
        return;
    }

    let dst_stride = dst_width as usize * 4;
    let required_dst_len = dst_stride.saturating_mul(dst_height as usize);
    if canvas.len() < required_dst_len {
        fill_black(canvas);
        return;
    }

    for row in 0..dst_height as usize {
        let src_start = row.saturating_mul(frame.stride);
        let src_end = src_start.saturating_add(dst_stride);
        let dst_start = row.saturating_mul(dst_stride);
        let dst_end = dst_start.saturating_add(dst_stride);
        if dst_start >= canvas.len() {
            break;
        }
        let safe_dst_end = dst_end.min(canvas.len());
        if src_end > frame.pixels.len() || dst_end > canvas.len() {
            fill_black(&mut canvas[dst_start..safe_dst_end]);
            continue;
        }
        canvas[dst_start..dst_end].copy_from_slice(&frame.pixels[src_start..src_end]);
    }
}

fn configure_viewport_source(
    viewport: &WpViewport,
    source_size: Option<(u32, u32)>,
    logical_width: u32,
    logical_height: u32,
    scale_mode: ScaleMode,
) {
    let Some((source_width_u32, source_height_u32)) = source_size else {
        viewport.set_source(0.0, 0.0, 1.0, 1.0);
        return;
    };

    let source_width = source_width_u32.max(1) as f64;
    let source_height = source_height_u32.max(1) as f64;
    if !source_width.is_finite() || !source_height.is_finite() {
        viewport.set_source(0.0, 0.0, 1.0, 1.0);
        return;
    }

    match scale_mode {
        ScaleMode::Fill => {
            let dst_width = logical_width.max(1) as f64;
            let dst_height = logical_height.max(1) as f64;
            let dst_aspect = dst_width / dst_height;
            let src_aspect = source_width / source_height;

            if src_aspect > dst_aspect {
                let crop_width = (source_height * dst_aspect).clamp(1.0, source_width);
                let crop_x = ((source_width - crop_width) * 0.5).max(0.0);
                viewport.set_source(crop_x, 0.0, crop_width, source_height);
            } else {
                let crop_height = (source_width / dst_aspect).clamp(1.0, source_height);
                let crop_y = ((source_height - crop_height) * 0.5).max(0.0);
                viewport.set_source(0.0, crop_y, source_width, crop_height);
            }
        }
        ScaleMode::Stretch | ScaleMode::Fit => {
            viewport.set_source(0.0, 0.0, source_width, source_height);
        }
    }
}

impl CompositorHandler for LayerWallpaperState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if let Some(index) = self
            .surfaces
            .iter()
            .position(|entry| entry.layer.wl_surface() == surface)
        {
            self.surfaces[index].scale_factor = new_factor.max(1);
            self.surfaces[index].buffer = None;
            self.surfaces[index].buffer_width = 0;
            self.surfaces[index].buffer_height = 0;
            self.surfaces[index].dmabuf_buffers.clear();
            self.surfaces[index].imported_dmabuf_frames.clear();
            if let Err(error) = self.draw_surface(qh, index) {
                self.fatal_error = Some(format!("scale-factor redraw failed: {error}"));
                self.exit = true;
                self.stop.store(true, Ordering::Relaxed);
            }
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_transform: wl_output::Transform,
    ) {
        if let Some(index) = self
            .surfaces
            .iter()
            .position(|entry| entry.layer.wl_surface() == surface)
        {
            self.surfaces[index].transform = new_transform;
            self.surfaces[index].buffer = None;
            self.surfaces[index].buffer_width = 0;
            self.surfaces[index].buffer_height = 0;
            self.surfaces[index].dmabuf_buffers.clear();
            self.surfaces[index].imported_dmabuf_frames.clear();
            if let Err(error) = self.draw_surface(qh, index) {
                self.fatal_error = Some(format!("transform redraw failed: {error}"));
                self.exit = true;
                self.stop.store(true, Ordering::Relaxed);
            }
        }
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if let Some(index) = self
            .surfaces
            .iter()
            .position(|entry| entry.layer.wl_surface() == surface)
            && let Err(error) = self.draw_surface(qh, index)
        {
            self.fatal_error = Some(format!("render failed: {error}"));
            self.exit = true;
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for LayerWallpaperState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for LayerWallpaperState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
        self.stop.store(true, Ordering::Relaxed);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if let Some(index) = self.surfaces.iter().position(|entry| entry.layer == *layer) {
            let width = configure.new_size.0.max(1);
            let height = configure.new_size.1.max(1);

            {
                let surface = &mut self.surfaces[index];
                if surface.width != width || surface.height != height {
                    surface.width = width;
                    surface.height = height;
                    surface.buffer = None;
                    surface.buffer_width = 0;
                    surface.buffer_height = 0;
                    surface.dmabuf_buffers.clear();
                    surface.imported_dmabuf_frames.clear();
                }
                if surface.first_configure {
                    surface.first_configure = false;
                }
            }

            if let Err(error) = self.draw_surface(qh, index) {
                self.fatal_error = Some(format!("configure redraw failed: {error}"));
                self.exit = true;
                self.stop.store(true, Ordering::Relaxed);
            }
        }
    }
}

impl ShmHandler for LayerWallpaperState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl DmabufHandler for LayerWallpaperState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_feedback(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _proxy: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        _feedback: DmabufFeedback,
    ) {
    }

    fn created(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        _buffer: wl_buffer::WlBuffer,
    ) {
    }

    fn failed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
    ) {
        if self.dmabuf_required {
            self.fatal_error = Some("dmabuf buffer creation failed".to_string());
            self.exit = true;
            self.stop.store(true, Ordering::Relaxed);
            return;
        }
        eprintln!("waybg renderer: dmabuf create failed, disabling dmabuf path.");
        self.disable_dmabuf();
    }

    fn released(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        buffer: &wl_buffer::WlBuffer,
    ) {
        let mut redraw_surface = None;
        for (surface_index, surface) in self.surfaces.iter_mut().enumerate() {
            if let Some(imported_index) = surface
                .imported_dmabuf_frames
                .iter()
                .position(|entry| entry.wl_buffer == *buffer)
            {
                surface.imported_dmabuf_frames.swap_remove(imported_index);
                redraw_surface = Some(surface_index);
                break;
            }
            if let Some(dmabuf) = surface
                .dmabuf_buffers
                .iter_mut()
                .find(|entry| entry.wl_buffer == *buffer)
            {
                dmabuf.released = true;
                redraw_surface = Some(surface_index);
                break;
            }
        }

        if let Some(surface_index) = redraw_surface
            && !self.exit
            && !self.stop.load(Ordering::Relaxed)
            && let Err(error) = self.draw_surface(qh, surface_index)
        {
            self.fatal_error = Some(format!("dmabuf release redraw failed: {error}"));
            self.exit = true;
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

delegate_compositor!(LayerWallpaperState);
delegate_output!(LayerWallpaperState);
delegate_shm!(LayerWallpaperState);
delegate_layer!(LayerWallpaperState);
delegate_simple!(LayerWallpaperState, WpViewporter, 1);
smithay_client_toolkit::delegate_dmabuf!(LayerWallpaperState);
delegate_registry!(LayerWallpaperState);

impl ProvidesRegistryState for LayerWallpaperState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

impl Dispatch<WpViewport, ()> for LayerWallpaperState {
    fn event(
        _: &mut LayerWallpaperState,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<LayerWallpaperState>,
    ) {
        unreachable!("wp_viewport::Event is empty in version 1");
    }
}

fn fill_black(canvas: &mut [u8]) {
    for pixel in canvas.chunks_exact_mut(4) {
        pixel[0] = 0;
        pixel[1] = 0;
        pixel[2] = 0;
        pixel[3] = 255;
    }
}

fn transform_swaps_axes(transform: wl_output::Transform) -> bool {
    matches!(
        transform,
        wl_output::Transform::Flipped90
            | wl_output::Transform::Flipped270
            | wl_output::Transform::_90
            | wl_output::Transform::_270
    )
}

fn blit_scaled_bgra(
    frame: &VideoFrame,
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    scale_mode: ScaleMode,
) {
    if frame.width == 0 || frame.height == 0 || dst_width == 0 || dst_height == 0 {
        fill_black(dst);
        return;
    }

    let dst_stride = dst_width as usize * 4;
    let needed_dst_len = dst_stride.saturating_mul(dst_height as usize);
    if dst.len() < needed_dst_len {
        fill_black(dst);
        return;
    }

    if matches!(scale_mode, ScaleMode::Stretch)
        && frame.width == dst_width
        && frame.height == dst_height
        && frame.stride == dst_stride
    {
        let src_needed = frame.stride.saturating_mul(frame.height as usize);
        if frame.pixels.len() >= src_needed {
            dst[..needed_dst_len].copy_from_slice(&frame.pixels[..needed_dst_len]);
            return;
        }
    }

    fill_black(dst);

    let src_width = frame.width as f64;
    let src_height = frame.height as f64;
    let dst_width_f = dst_width as f64;
    let dst_height_f = dst_height as f64;

    let (scale_x, scale_y) = match scale_mode {
        ScaleMode::Stretch => (dst_width_f / src_width, dst_height_f / src_height),
        ScaleMode::Fit => {
            let scale = (dst_width_f / src_width).min(dst_height_f / src_height);
            (scale, scale)
        }
        ScaleMode::Fill => {
            let scale = (dst_width_f / src_width).max(dst_height_f / src_height);
            (scale, scale)
        }
    };
    if scale_x <= 0.0 || scale_y <= 0.0 {
        return;
    }

    let scaled_width = src_width * scale_x;
    let scaled_height = src_height * scale_y;
    let offset_x = (dst_width_f - scaled_width) * 0.5;
    let offset_y = (dst_height_f - scaled_height) * 0.5;

    for y in 0..dst_height as usize {
        let dst_row = y.saturating_mul(dst_stride);
        let y_center = y as f64 + 0.5;

        for x in 0..dst_width as usize {
            let dst_index = dst_row + x.saturating_mul(4);
            if dst_index + 4 > dst.len() {
                continue;
            }
            let x_center = x as f64 + 0.5;

            if matches!(scale_mode, ScaleMode::Fit)
                && (x_center < offset_x
                    || x_center >= offset_x + scaled_width
                    || y_center < offset_y
                    || y_center >= offset_y + scaled_height)
            {
                continue;
            }

            let src_x = ((x_center - offset_x) / scale_x) - 0.5;
            let src_y = ((y_center - offset_y) / scale_y) - 0.5;
            let sample = sample_bilinear_bgra(frame, src_x, src_y);
            dst[dst_index..dst_index + 4].copy_from_slice(&sample);
        }
    }
}

fn sample_bilinear_bgra(frame: &VideoFrame, src_x: f64, src_y: f64) -> [u8; 4] {
    let max_x = frame.width.saturating_sub(1) as f64;
    let max_y = frame.height.saturating_sub(1) as f64;

    let x = src_x.clamp(0.0, max_x);
    let y = src_y.clamp(0.0, max_y);

    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = x0.saturating_add(1).min(frame.width.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(frame.height.saturating_sub(1));

    let tx = x - x0 as f64;
    let ty = y - y0 as f64;

    let mut out = [0u8; 4];
    for (channel, out_channel) in out.iter_mut().enumerate() {
        let p00 = pixel_bgra(frame, x0, y0, channel) as f64;
        let p10 = pixel_bgra(frame, x1, y0, channel) as f64;
        let p01 = pixel_bgra(frame, x0, y1, channel) as f64;
        let p11 = pixel_bgra(frame, x1, y1, channel) as f64;

        let top = p00 + (p10 - p00) * tx;
        let bottom = p01 + (p11 - p01) * tx;
        let value = top + (bottom - top) * ty;
        *out_channel = value.round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn pixel_bgra(frame: &VideoFrame, x: u32, y: u32, channel: usize) -> u8 {
    let index = y as usize * frame.stride + x as usize * 4 + channel;
    frame.pixels.get(index).copied().unwrap_or(0)
}

fn try_pull_sample(appsink: &gst::Element) -> Option<gst::Sample> {
    appsink.emit_by_name::<Option<gst::Sample>>("try-pull-sample", &[&0u64])
}

fn build_appsink_caps(dmabuf_mode: DmabufMode) -> gst::Caps {
    let bgra_dmabuf = gst::Structure::builder("video/x-raw")
        .field("format", "BGRA")
        .build();
    let dma_drm = gst::Structure::builder("video/x-raw")
        .field("format", "DMA_DRM")
        .build();
    let bgra_cpu = gst::Structure::builder("video/x-raw")
        .field("format", "BGRA")
        .build();
    let dmabuf_features = gst::CapsFeatures::new([GST_CAPS_FEATURE_MEMORY_DMABUF]);

    match dmabuf_mode {
        DmabufMode::Off => gst::Caps::builder("video/x-raw")
            .field("format", "BGRA")
            .build(),
        DmabufMode::On => gst::Caps::builder_full()
            .structure_with_features(dma_drm, dmabuf_features.clone())
            .structure_with_features(bgra_dmabuf, dmabuf_features)
            .build(),
        DmabufMode::Auto => gst::Caps::builder_full()
            .structure_with_features(dma_drm, dmabuf_features.clone())
            .structure_with_features(bgra_dmabuf, dmabuf_features)
            .structure(bgra_cpu)
            .build(),
    }
}

fn sample_to_frame_payload(
    sample: gst::Sample,
    allow_dmabuf: bool,
) -> Result<FramePayload, io::Error> {
    if allow_dmabuf && let Ok(dmabuf_frame) = sample_to_dmabuf_frame(sample.clone()) {
        return Ok(FramePayload::Dmabuf(Arc::new(dmabuf_frame)));
    }

    let cpu_frame = sample_to_video_frame(&sample)?;
    Ok(FramePayload::Cpu(Arc::new(cpu_frame)))
}

fn sample_to_dmabuf_frame(sample: gst::Sample) -> Result<DmabufVideoFrame, io::Error> {
    let caps = sample
        .caps()
        .ok_or_else(|| io::Error::other("sample is missing caps"))?;
    let structure = caps
        .structure(0)
        .ok_or_else(|| io::Error::other("caps have no first structure"))?;
    let width = structure
        .get::<i32>("width")
        .map_err(|error| io::Error::other(format!("failed to read sample width: {error}")))?
        .max(1) as u32;
    let height = structure
        .get::<i32>("height")
        .map_err(|error| io::Error::other(format!("failed to read sample height: {error}")))?
        .max(1) as u32;
    let format_name = structure
        .get::<String>("format")
        .map_err(|error| io::Error::other(format!("failed to read sample format: {error}")))?;

    let buffer = sample
        .buffer()
        .ok_or_else(|| io::Error::other("sample is missing buffer"))?;

    let is_dma_drm = format_name.eq_ignore_ascii_case("DMA_DRM");
    let (drm_format, modifier, bytes_per_pixel) = if is_dma_drm {
        let drm_format_string = structure.get::<String>("drm-format").map_err(|error| {
            io::Error::other(format!(
                "failed to read DMA_DRM drm-format field from caps: {error}"
            ))
        })?;
        let (fourcc, modifier) = drm_fourcc_and_modifier_from_caps_string(&drm_format_string)?;
        (fourcc, modifier, None)
    } else {
        let (drm_format, bytes_per_pixel) = drm_format_from_gst_video_format(&format_name)
            .ok_or_else(|| {
                io::Error::other(format!("unsupported dmabuf format '{format_name}'"))
            })?;
        let modifier = dmabuf_modifier_from_caps(caps).unwrap_or(DRM_FORMAT_MOD_LINEAR);
        (drm_format, modifier, Some(bytes_per_pixel))
    };

    let video_meta = buffer_video_meta(buffer);
    let n_planes = if let Some(meta) = video_meta {
        normalize_plane_count(meta.n_planes as usize)?
    } else if is_dma_drm {
        return Err(io::Error::other(
            "DMA_DRM sample is missing GstVideoMeta, cannot resolve plane layout",
        ));
    } else {
        1
    };

    let planes =
        collect_dmabuf_planes(buffer, video_meta, width, height, n_planes, bytes_per_pixel)?;

    Ok(DmabufVideoFrame {
        width,
        height,
        format: drm_format,
        modifier,
        planes,
        sample,
    })
}

fn dmabuf_frame_to_video_frame(frame: &DmabufVideoFrame) -> Option<VideoFrame> {
    if frame.planes.len() != 1 {
        return None;
    }
    if frame.format != DRM_FORMAT_ARGB8888 && frame.format != DRM_FORMAT_XRGB8888 {
        return None;
    }

    let buffer = frame.sample.buffer()?;
    let map = buffer.map_readable().ok()?;
    let data = map.as_slice();
    let height = frame.height as usize;
    if height == 0 || data.len() < height {
        return None;
    }
    let stride = frame.planes[0].stride as usize;
    let min_required = stride.saturating_mul(height);
    if data.len() < min_required {
        return None;
    }
    let min_stride = frame.width as usize * 4;
    if stride < min_stride {
        return None;
    }

    Some(VideoFrame {
        width: frame.width,
        height: frame.height,
        stride,
        pixels: data[..min_required].to_vec(),
    })
}

fn normalize_plane_count(n_planes: usize) -> Result<usize, io::Error> {
    if n_planes == 0 || n_planes > GST_VIDEO_MAX_PLANES {
        return Err(io::Error::other(format!(
            "invalid dmabuf plane count {n_planes}"
        )));
    }
    Ok(n_planes)
}

fn collect_dmabuf_planes(
    buffer: &gst::BufferRef,
    video_meta: Option<&GstVideoMetaPrefix>,
    width: u32,
    height: u32,
    n_planes: usize,
    bytes_per_pixel: Option<usize>,
) -> Result<Vec<DmabufPlane>, io::Error> {
    let n_memory = buffer.n_memory();
    if n_memory == 0 {
        return Err(io::Error::other("sample buffer has no memories"));
    }

    if n_memory == 1 {
        let memory = buffer.peek_memory(0);
        let raw_fd = dmabuf_memory_fd(memory)?;
        let mut planes = Vec::with_capacity(n_planes);
        let plane_stride_fallback = if n_planes == 1 && video_meta.is_none() {
            Some(calculate_single_plane_stride(
                memory.size(),
                width,
                height,
                bytes_per_pixel,
            )?)
        } else {
            None
        };

        for plane_index in 0..n_planes {
            let (offset, stride) = if let Some(meta) = video_meta {
                plane_layout_from_meta(meta, plane_index)?
            } else {
                (
                    0,
                    plane_stride_fallback.ok_or_else(|| {
                        io::Error::other("missing plane metadata for dmabuf import")
                    })?,
                )
            };
            planes.push(DmabufPlane {
                fd: dup_fd_cloexec(raw_fd)?,
                offset,
                stride,
            });
        }
        return Ok(planes);
    }

    if n_memory == n_planes {
        let mut planes = Vec::with_capacity(n_planes);
        for plane_index in 0..n_planes {
            let memory = buffer.peek_memory(plane_index);
            let raw_fd = dmabuf_memory_fd(memory)?;
            let stride = if let Some(meta) = video_meta {
                let (_, stride) = plane_layout_from_meta(meta, plane_index)?;
                stride
            } else if n_planes == 1 {
                calculate_single_plane_stride(memory.size(), width, height, bytes_per_pixel)?
            } else {
                return Err(io::Error::other(
                    "multi-memory dmabuf sample missing GstVideoMeta stride data",
                ));
            };
            planes.push(DmabufPlane {
                fd: dup_fd_cloexec(raw_fd)?,
                offset: 0,
                stride,
            });
        }
        return Ok(planes);
    }

    Err(io::Error::other(format!(
        "unsupported dmabuf memory layout: {n_memory} memories for {n_planes} planes"
    )))
}

fn dmabuf_memory_fd(memory: &gst::MemoryRef) -> Result<i32, io::Error> {
    if !memory.is_type(GST_MEMORY_TYPE_DMABUF) {
        return Err(io::Error::other("sample memory is not dmabuf"));
    }
    dmabuf_memory_get_fd(memory)
}

fn calculate_single_plane_stride(
    total_size: usize,
    width: u32,
    height: u32,
    bytes_per_pixel: Option<usize>,
) -> Result<u32, io::Error> {
    let height_usize = height as usize;
    if height_usize == 0 || total_size < height_usize {
        return Err(io::Error::other("invalid dmabuf plane dimensions"));
    }
    if !total_size.is_multiple_of(height_usize) {
        return Err(io::Error::other(format!(
            "dmabuf plane size {total_size} is not divisible by frame height {height}"
        )));
    }
    let stride = (total_size / height_usize) as u32;
    if let Some(bytes_per_pixel) = bytes_per_pixel {
        let min_stride = (bytes_per_pixel as u32).saturating_mul(width);
        if stride < min_stride {
            return Err(io::Error::other(format!(
                "dmabuf stride ({stride}) is smaller than required stride ({min_stride})"
            )));
        }
    }
    Ok(stride)
}

fn plane_layout_from_meta(
    meta: &GstVideoMetaPrefix,
    plane_index: usize,
) -> Result<(u32, u32), io::Error> {
    if plane_index >= GST_VIDEO_MAX_PLANES {
        return Err(io::Error::other(format!(
            "plane index {plane_index} is out of range"
        )));
    }
    let offset = u32::try_from(meta.offset[plane_index]).map_err(|_| {
        io::Error::other(format!(
            "dmabuf plane offset {} does not fit into u32",
            meta.offset[plane_index]
        ))
    })?;
    let stride = u32::try_from(meta.stride[plane_index]).map_err(|_| {
        io::Error::other(format!(
            "dmabuf plane stride {} is invalid",
            meta.stride[plane_index]
        ))
    })?;
    Ok((offset, stride))
}

fn buffer_video_meta(buffer: &gst::BufferRef) -> Option<&GstVideoMetaPrefix> {
    let ptr = unsafe { gst_buffer_get_video_meta(buffer.as_ptr() as *mut gst::ffi::GstBuffer) };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn drm_fourcc_and_modifier_from_caps_string(value: &str) -> Result<(u32, u64), io::Error> {
    let c_value = CString::new(value).map_err(|error| {
        io::Error::other(format!(
            "invalid drm-format string '{value}': contains interior NUL: {error}"
        ))
    })?;
    let mut modifier = 0u64;
    let fourcc = unsafe { gst_video_dma_drm_fourcc_from_string(c_value.as_ptr(), &mut modifier) };
    if fourcc == 0 {
        return Err(io::Error::other(format!(
            "failed to parse DRM fourcc/modifier from '{value}'"
        )));
    }
    Ok((fourcc, modifier))
}

fn sample_to_video_frame(sample: &gst::Sample) -> Result<VideoFrame, io::Error> {
    let caps = sample
        .caps()
        .ok_or_else(|| io::Error::other("sample is missing caps"))?;
    let structure = caps
        .structure(0)
        .ok_or_else(|| io::Error::other("caps have no first structure"))?;
    let width = structure
        .get::<i32>("width")
        .map_err(|error| io::Error::other(format!("failed to read sample width: {error}")))?
        .max(1) as u32;
    let height = structure
        .get::<i32>("height")
        .map_err(|error| io::Error::other(format!("failed to read sample height: {error}")))?
        .max(1) as u32;

    let buffer = sample
        .buffer()
        .ok_or_else(|| io::Error::other("sample is missing buffer"))?;
    let map = buffer
        .map_readable()
        .map_err(|_| io::Error::other("failed to map sample buffer"))?;
    let data = map.as_slice();
    let stride = data.len() / height as usize;
    let min_stride = width as usize * 4;
    if stride < min_stride {
        return Err(io::Error::other(format!(
            "sample stride ({stride}) is smaller than required BGRA stride ({min_stride})"
        )));
    }

    Ok(VideoFrame {
        width,
        height,
        stride,
        pixels: data.to_vec(),
    })
}

fn drm_format_from_gst_video_format(format_name: &str) -> Option<(u32, usize)> {
    match format_name.to_ascii_uppercase().as_str() {
        "BGRA" => Some((DRM_FORMAT_ARGB8888, 4)),
        "BGRX" => Some((DRM_FORMAT_XRGB8888, 4)),
        _ => None,
    }
}

fn dmabuf_modifier_from_caps(caps: &gst::CapsRef) -> Option<u64> {
    let structure = caps.structure(0)?;

    if let Ok(modifier) = structure.get::<u64>("modifier") {
        return Some(modifier);
    }
    if let Ok(modifier) = structure.get::<i64>("modifier")
        && modifier >= 0
    {
        return Some(modifier as u64);
    }
    if let Ok(drm_format) = structure.get::<String>("drm-format") {
        return parse_drm_format_modifier(&drm_format);
    }

    None
}

fn parse_drm_format_modifier(value: &str) -> Option<u64> {
    let (_, modifier) = value.split_once(':')?;
    if let Some(stripped) = modifier
        .strip_prefix("0x")
        .or_else(|| modifier.strip_prefix("0X"))
    {
        return u64::from_str_radix(stripped, 16).ok();
    }
    modifier.parse::<u64>().ok()
}

fn resolve_dmabuf_mode() -> Result<DmabufMode, io::Error> {
    if let Some(raw_value) = env::var_os(WAYBG_DMABUF_ENV) {
        let value = raw_value.to_string_lossy();
        return parse_dmabuf_mode(value.trim());
    }
    parse_dmabuf_mode(DMABUF_MODE_AUTO)
}

fn parse_dmabuf_mode(value: &str) -> Result<DmabufMode, io::Error> {
    match value.to_ascii_lowercase().as_str() {
        "" | DMABUF_MODE_AUTO => Ok(DmabufMode::Auto),
        DMABUF_MODE_ON | "true" | "1" | "yes" => Ok(DmabufMode::On),
        DMABUF_MODE_OFF | "false" | "0" | "no" => Ok(DmabufMode::Off),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid WAYBG_DMABUF value '{other}', expected one of: {DMABUF_MODE_AUTO}, {DMABUF_MODE_ON}, {DMABUF_MODE_OFF}"
            ),
        )),
    }
}

#[cfg(target_os = "linux")]
#[link(name = "gstallocators-1.0")]
unsafe extern "C" {
    fn gst_dmabuf_memory_get_fd(memory: *mut gst::ffi::GstMemory) -> libc::c_int;
}

#[cfg(target_os = "linux")]
#[link(name = "gstvideo-1.0")]
unsafe extern "C" {
    fn gst_buffer_get_video_meta(buffer: *mut gst::ffi::GstBuffer) -> *mut GstVideoMetaPrefix;
    fn gst_video_dma_drm_fourcc_from_string(
        format_str: *const libc::c_char,
        modifier: *mut u64,
    ) -> u32;
}

#[cfg(not(target_os = "linux"))]
unsafe fn gst_buffer_get_video_meta(_buffer: *mut gst::ffi::GstBuffer) -> *mut GstVideoMetaPrefix {
    std::ptr::null_mut()
}

#[cfg(not(target_os = "linux"))]
unsafe fn gst_video_dma_drm_fourcc_from_string(
    _format_str: *const libc::c_char,
    _modifier: *mut u64,
) -> u32 {
    0
}

#[cfg(target_os = "linux")]
fn dmabuf_memory_get_fd(memory: &gst::MemoryRef) -> Result<i32, io::Error> {
    let fd = unsafe { gst_dmabuf_memory_get_fd(memory.as_ptr() as *mut gst::ffi::GstMemory) };
    if fd < 0 {
        Err(io::Error::other(
            "gst_dmabuf_memory_get_fd returned an invalid fd",
        ))
    } else {
        Ok(fd)
    }
}

#[cfg(not(target_os = "linux"))]
fn dmabuf_memory_get_fd(_memory: &gst::MemoryRef) -> Result<i32, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "dmabuf decode import is only supported on Linux",
    ))
}

fn dup_fd_cloexec(fd: i32) -> Result<OwnedFd, io::Error> {
    let duplicated_fd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicated_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated_fd) })
}

fn open_dma_heap_device() -> Result<OwnedFd, io::Error> {
    let mut last_error = None;
    for candidate in DMA_HEAP_DEVICE_CANDIDATES {
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(candidate)
        {
            Ok(file) => return Ok(file.into()),
            Err(error) => last_error = Some((candidate, error)),
        }
    }

    if let Some((path, error)) = last_error {
        Err(io::Error::new(
            error.kind(),
            format!("failed to open any dma_heap device (last attempted '{path}'): {error}"),
        ))
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no dma_heap devices configured",
        ))
    }
}

fn dma_heap_alloc_fd(heap_fd: &OwnedFd, len: usize) -> Result<OwnedFd, io::Error> {
    let mut request = DmaHeapAllocationData {
        len: len as u64,
        fd: 0,
        fd_flags: (libc::O_RDWR | libc::O_CLOEXEC) as u32,
        heap_flags: 0,
    };
    let result = unsafe { libc::ioctl(heap_fd.as_raw_fd(), dma_heap_ioctl_alloc(), &mut request) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }

    let raw_fd = request.fd as i32;
    if raw_fd < 0 {
        return Err(io::Error::other(
            "dma_heap returned an invalid file descriptor",
        ));
    }

    Ok(unsafe { OwnedFd::from_raw_fd(raw_fd) })
}

fn align_up(value: usize, align: usize) -> usize {
    if align <= 1 {
        return value;
    }
    let remainder = value % align;
    if remainder == 0 {
        value
    } else {
        value.saturating_add(align - remainder)
    }
}

const fn dma_heap_ioctl_alloc() -> libc::c_ulong {
    const IOC_NRBITS: u64 = 8;
    const IOC_TYPEBITS: u64 = 8;
    const IOC_SIZEBITS: u64 = 14;

    const IOC_NRSHIFT: u64 = 0;
    const IOC_TYPESHIFT: u64 = IOC_NRSHIFT + IOC_NRBITS;
    const IOC_SIZESHIFT: u64 = IOC_TYPESHIFT + IOC_TYPEBITS;
    const IOC_DIRSHIFT: u64 = IOC_SIZESHIFT + IOC_SIZEBITS;

    const IOC_WRITE: u64 = 1;
    const IOC_READ: u64 = 2;

    let dir = IOC_READ | IOC_WRITE;
    let size = std::mem::size_of::<DmaHeapAllocationData>() as u64;
    let request = (dir << IOC_DIRSHIFT)
        | ((b'H' as u64) << IOC_TYPESHIFT)
        | (0u64 << IOC_NRSHIFT)
        | (size << IOC_SIZESHIFT);
    request as libc::c_ulong
}

fn resolve_playback_backend() -> Result<PlaybackBackend, io::Error> {
    if let Some(raw_value) = env::var_os(WAYBG_BACKEND_ENV) {
        let value = raw_value.to_string_lossy();
        return parse_backend(value.trim());
    }
    parse_backend(BACKEND_AUTO)
}

fn resolve_scale_mode() -> Result<ScaleMode, io::Error> {
    if let Some(raw_value) = env::var_os(WAYBG_SCALE_MODE_ENV) {
        let value = raw_value.to_string_lossy();
        return parse_scale_mode(value.trim());
    }
    parse_scale_mode(SCALE_MODE_FILL)
}

fn parse_backend(value: &str) -> Result<PlaybackBackend, io::Error> {
    match value.to_ascii_lowercase().as_str() {
        "" | BACKEND_AUTO => {
            if is_niri_session() {
                Ok(PlaybackBackend::LayerShell)
            } else {
                Ok(PlaybackBackend::GstreamerWindow)
            }
        }
        BACKEND_GSTREAMER => Ok(PlaybackBackend::GstreamerWindow),
        BACKEND_LAYER_SHELL => Ok(PlaybackBackend::LayerShell),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid WAYBG_BACKEND value '{other}', expected one of: {BACKEND_AUTO}, {BACKEND_GSTREAMER}, {BACKEND_LAYER_SHELL}"
            ),
        )),
    }
}

fn parse_scale_mode(value: &str) -> Result<ScaleMode, io::Error> {
    match value.to_ascii_lowercase().as_str() {
        "" | SCALE_MODE_FILL | "cover" => Ok(ScaleMode::Fill),
        SCALE_MODE_FIT | "contain" => Ok(ScaleMode::Fit),
        SCALE_MODE_STRETCH => Ok(ScaleMode::Stretch),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid WAYBG_SCALE_MODE value '{other}', expected one of: {SCALE_MODE_FILL}, {SCALE_MODE_FIT}, {SCALE_MODE_STRETCH}"
            ),
        )),
    }
}

fn scale_mode_name(scale_mode: ScaleMode) -> &'static str {
    match scale_mode {
        ScaleMode::Fit => SCALE_MODE_FIT,
        ScaleMode::Fill => SCALE_MODE_FILL,
        ScaleMode::Stretch => SCALE_MODE_STRETCH,
    }
}

fn is_niri_session() -> bool {
    if env::var_os("NIRI_SOCKET").is_some() {
        return true;
    }

    for key in [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ] {
        if env::var(key)
            .ok()
            .is_some_and(|value| value.to_ascii_lowercase().contains("niri"))
        {
            return true;
        }
    }

    false
}

fn play_video_gstreamer_window(
    input: &str,
    loop_playback: bool,
    output: Option<&str>,
    mute: bool,
    metrics_file: Option<&Path>,
) -> Result<(), DynError> {
    let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    let _wayland_connection = Connection::connect_to_env().map_err(|error| {
        io::Error::other(format!(
            "failed to connect to Wayland display '{wayland_display}' via SCTK: {error}"
        ))
    })?;

    gst::init()
        .map_err(|error| io::Error::other(format!("failed to initialize GStreamer: {error}")))?;
    let hardware_decoders = configure_hardware_decoder_preference();

    warn_about_codec_runtime();

    if is_blank_source(input) {
        write_placeholder_metrics(
            metrics_file,
            BACKEND_GSTREAMER,
            input,
            output,
            &hardware_decoders,
            Some("blank source does not emit FPS samples"),
        );
        return play_blank_video(loop_playback, &wayland_display, output, mute);
    }

    write_placeholder_metrics(
        metrics_file,
        BACKEND_GSTREAMER,
        input,
        output,
        &hardware_decoders,
        Some(
            "FPS sampling is only available on layer-shell backend. Switch WAYBG_BACKEND=layer-shell for frame metrics.",
        ),
    );

    let uri = to_uri(input)?;

    let playbin = gst::ElementFactory::make("playbin")
        .name("player")
        .build()
        .map_err(|_| io::Error::other("GStreamer element 'playbin' is unavailable"))?;

    let waylandsink = gst::ElementFactory::make("waylandsink")
        .name("wallpaper_sink")
        .build()
        .map_err(|_| {
            io::Error::other(format!(
                "GStreamer element 'waylandsink' is unavailable. Install gst-plugins-bad with Wayland support. {ARCH_CODEC_HINT}"
            ))
        })?;
    apply_output_target(&waylandsink, output);

    playbin.set_property("video-sink", &waylandsink);
    playbin.set_property("uri", &uri);
    playbin.set_property("mute", mute);

    let bus = playbin
        .bus()
        .ok_or_else(|| io::Error::other("failed to retrieve GStreamer bus"))?;

    playbin.set_state(gst::State::Playing).map_err(|error| {
        io::Error::other(format!("failed to set pipeline to Playing: {error:?}"))
    })?;

    println!(
        "Playing on Wayland display '{wayland_display}': {uri} (loop={loop_playback}, output={}, mute={mute})",
        output.unwrap_or("<auto>")
    );

    for message in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match message.view() {
            MessageView::Eos(..) => {
                if loop_playback {
                    playbin
                        .seek_simple(
                            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                            gst::ClockTime::ZERO,
                        )
                        .map_err(|error| {
                            io::Error::other(format!(
                                "failed to seek to start for looped playback: {error}"
                            ))
                        })?;
                } else {
                    println!("End of stream.");
                    break;
                }
            }
            MessageView::Error(error) => {
                let source = error
                    .src()
                    .map(|src| src.path_string())
                    .unwrap_or_else(|| "unknown".into());
                return Err(io::Error::other(format!(
                    "GStreamer error from {source}: {} ({:?})",
                    error.error(),
                    error.debug()
                ))
                .into());
            }
            _ => {}
        }
    }

    playbin
        .set_state(gst::State::Null)
        .map_err(|error| io::Error::other(format!("failed to set pipeline to Null: {error:?}")))?;

    Ok(())
}

fn is_blank_source(input: &str) -> bool {
    let normalized = input.trim().to_ascii_lowercase();
    normalized == "blank" || normalized == "none" || normalized == BLANK_VIDEO_URI
}

fn configure_hardware_decoder_preference() -> Vec<String> {
    let candidates = [
        "v4l2slh264dec",
        "v4l2slh265dec",
        "v4l2slvp9dec",
        "v4l2slav1dec",
        "v4l2h264dec",
        "v4l2h265dec",
        "v4l2vp9dec",
        "v4l2av1dec",
        "vah264dec",
        "vah265dec",
        "vavp9dec",
        "vaav1dec",
        "vaapih264dec",
        "vaapih265dec",
        "vaapivp9dec",
        "nvh264dec",
        "nvh265dec",
        "nvav1dec",
        "d3d11h264dec",
        "d3d11h265dec",
        "d3d11vp9dec",
        "d3d11av1dec",
        "qsvh264dec",
        "qsvh265dec",
        "vtdec",
    ];
    let preferred_rank = gst::Rank::PRIMARY + 512;
    let mut enabled = Vec::new();

    for candidate in candidates {
        if let Some(factory) = gst::ElementFactory::find(candidate) {
            if factory.rank() < preferred_rank {
                factory.set_rank(preferred_rank);
            }
            enabled.push(candidate.to_string());
        }
    }

    if enabled.is_empty() {
        eprintln!(
            "Hardware decode preference: no known hardware decoders detected, using default decoder selection."
        );
    } else {
        println!(
            "Hardware decode preference enabled for {} decoder(s): {}",
            enabled.len(),
            enabled.join(", ")
        );
    }

    enabled
}

fn warn_about_codec_runtime() {
    let has_ffmpeg_bridge = ["avdec_h264", "avdec_hevc", "avdec_vp9", "avdec_av1"]
        .iter()
        .any(|decoder| gst::ElementFactory::find(decoder).is_some());
    let has_av1_decoder = ["dav1ddec", "av1dec", "avdec_av1"]
        .iter()
        .any(|decoder| gst::ElementFactory::find(decoder).is_some());

    if !has_ffmpeg_bridge || !has_av1_decoder {
        eprintln!("{ARCH_CODEC_HINT}");
        if !has_ffmpeg_bridge {
            eprintln!("Codec runtime warning: no gst-libav ffmpeg decoder was detected.");
        }
        if !has_av1_decoder {
            eprintln!(
                "Codec runtime warning: no AV1 decoder detected (`dav1ddec`, `av1dec`, or `avdec_av1`)."
            );
        }
    }
}

fn play_blank_video(
    loop_playback: bool,
    wayland_display: &str,
    output: Option<&str>,
    mute: bool,
) -> Result<(), DynError> {
    let source = gst::ElementFactory::make("videotestsrc")
        .name("blank_src")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'videotestsrc' is unavailable. Install gst-plugins-base.",
            )
        })?;
    source.set_property_from_str("pattern", "black");
    source.set_property("is-live", true);
    if !loop_playback {
        source.set_property("num-buffers", 1u32);
    }

    let convert = gst::ElementFactory::make("videoconvert")
        .name("blank_convert")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'videoconvert' is unavailable. Install gst-plugins-base.",
            )
        })?;

    let sink = gst::ElementFactory::make("waylandsink")
        .name("blank_sink")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'waylandsink' is unavailable. Install gst-plugins-bad with Wayland support.",
            )
        })?;
    apply_output_target(&sink, output);

    let pipeline = gst::Pipeline::new();
    pipeline
        .add_many([&source, &convert, &sink])
        .map_err(|error| io::Error::other(format!("failed to build blank pipeline: {error}")))?;

    gst::Element::link_many([&source, &convert, &sink])
        .map_err(|error| io::Error::other(format!("failed to link blank pipeline: {error}")))?;

    let bus = pipeline
        .bus()
        .ok_or_else(|| io::Error::other("failed to retrieve GStreamer bus"))?;

    pipeline.set_state(gst::State::Playing).map_err(|error| {
        io::Error::other(format!("failed to set pipeline to Playing: {error:?}"))
    })?;

    println!(
        "Playing blank background on Wayland display '{wayland_display}' (loop={loop_playback}, output={}, mute={mute})",
        output.unwrap_or("<auto>")
    );

    for message in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match message.view() {
            MessageView::Eos(..) => {
                break;
            }
            MessageView::Error(error) => {
                let source = error
                    .src()
                    .map(|src| src.path_string())
                    .unwrap_or_else(|| "unknown".into());
                return Err(io::Error::other(format!(
                    "GStreamer error from {source}: {} ({:?})",
                    error.error(),
                    error.debug()
                ))
                .into());
            }
            _ => {}
        }
    }

    pipeline
        .set_state(gst::State::Null)
        .map_err(|error| io::Error::other(format!("failed to set pipeline to Null: {error:?}")))?;

    Ok(())
}

fn to_uri(input: &str) -> Result<String, io::Error> {
    if input.contains("://") {
        return Ok(input.to_string());
    }

    let input_path = PathBuf::from(input);
    let absolute_path = if input_path.is_absolute() {
        input_path
    } else {
        env::current_dir()?.join(input_path)
    };

    let normalized_path = absolute_path
        .canonicalize()
        .unwrap_or_else(|_| absolute_path.clone());

    gst::glib::filename_to_uri(&normalized_path, None)
        .map(|uri| uri.to_string())
        .map_err(|error| {
            io::Error::other(format!(
                "failed to convert '{}' into a file URI: {error}",
                normalized_path.display()
            ))
        })
}

fn apply_output_target(sink: &gst::Element, output: Option<&str>) {
    if let Some(output_name) = output.map(str::trim).filter(|name| !name.is_empty()) {
        if sink.find_property("fullscreen").is_some() {
            sink.set_property("fullscreen", true);
        }

        if sink.find_property("fullscreen-output").is_some() {
            sink.set_property("fullscreen-output", output_name);
            return;
        }

        if sink.find_property("output").is_some() {
            sink.set_property("output", output_name);
            return;
        }

        eprintln!(
            "warning: requested output '{output_name}', but waylandsink does not expose a supported output target property"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use super::{
        DmabufMode, GST_CAPS_FEATURE_MEMORY_DMABUF, PlaybackBackend, ScaleMode, build_appsink_caps,
        is_blank_source, mean_fps, parse_backend, parse_dmabuf_mode, parse_scale_mode,
        percentile_low_fps,
    };

    fn ensure_gstreamer_init() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            super::gst::init().expect("gstreamer should initialize for tests");
        });
    }

    #[test]
    fn blank_source_aliases_are_supported() {
        assert!(is_blank_source("blank"));
        assert!(is_blank_source("blank://"));
        assert!(is_blank_source("none"));
        assert!(!is_blank_source("video.mp4"));
    }

    #[test]
    fn backend_parser_accepts_expected_values() {
        assert_eq!(
            parse_backend("gstreamer").expect("valid backend"),
            PlaybackBackend::GstreamerWindow
        );
        assert_eq!(
            parse_backend("layer-shell").expect("valid backend"),
            PlaybackBackend::LayerShell
        );
    }

    #[test]
    fn backend_parser_rejects_unknown_value() {
        let error = parse_backend("unknown")
            .expect_err("invalid backend should fail")
            .to_string();
        assert!(error.contains("invalid WAYBG_BACKEND value"));
    }

    #[test]
    fn scale_mode_parser_accepts_expected_values() {
        assert_eq!(
            parse_scale_mode("fill").expect("valid scale mode"),
            ScaleMode::Fill
        );
        assert_eq!(
            parse_scale_mode("fit").expect("valid scale mode"),
            ScaleMode::Fit
        );
        assert_eq!(
            parse_scale_mode("stretch").expect("valid scale mode"),
            ScaleMode::Stretch
        );
    }

    #[test]
    fn scale_mode_parser_rejects_unknown_value() {
        let error = parse_scale_mode("bad")
            .expect_err("invalid scale mode should fail")
            .to_string();
        assert!(error.contains("invalid WAYBG_SCALE_MODE value"));
    }

    #[test]
    fn dmabuf_mode_parser_accepts_expected_values() {
        assert_eq!(
            parse_dmabuf_mode("auto").expect("valid dmabuf mode"),
            DmabufMode::Auto
        );
        assert_eq!(
            parse_dmabuf_mode("on").expect("valid dmabuf mode"),
            DmabufMode::On
        );
        assert_eq!(
            parse_dmabuf_mode("off").expect("valid dmabuf mode"),
            DmabufMode::Off
        );
    }

    #[test]
    fn dmabuf_mode_parser_rejects_unknown_value() {
        let error = parse_dmabuf_mode("maybe")
            .expect_err("invalid dmabuf mode should fail")
            .to_string();
        assert!(error.contains("invalid WAYBG_DMABUF value"));
    }

    #[test]
    fn mean_fps_uses_arithmetic_average() {
        let samples = [30.0, 60.0, 90.0];
        assert!((mean_fps(&samples) - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn low_percentiles_favor_worst_tail_values() {
        let samples = [120.0, 90.0, 60.0, 45.0, 30.0];
        assert!((percentile_low_fps(&samples, 0.95) - 30.0).abs() < f64::EPSILON);
        assert!((percentile_low_fps(&samples, 0.99) - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn appsink_caps_prioritize_dma_drm_for_dmabuf_modes() {
        ensure_gstreamer_init();

        let on_caps = build_appsink_caps(DmabufMode::On);
        assert_eq!(on_caps.size(), 2);
        assert_eq!(
            on_caps
                .structure(0)
                .and_then(|s| s.get::<String>("format").ok())
                .as_deref(),
            Some("DMA_DRM")
        );
        assert_eq!(
            on_caps
                .structure(1)
                .and_then(|s| s.get::<String>("format").ok())
                .as_deref(),
            Some("BGRA")
        );
        assert!(
            on_caps
                .features(0)
                .expect("first structure should have caps features")
                .contains(GST_CAPS_FEATURE_MEMORY_DMABUF)
        );
        assert!(
            on_caps
                .features(1)
                .expect("second structure should have caps features")
                .contains(GST_CAPS_FEATURE_MEMORY_DMABUF)
        );

        let auto_caps = build_appsink_caps(DmabufMode::Auto);
        assert_eq!(auto_caps.size(), 3);
        assert_eq!(
            auto_caps
                .structure(0)
                .and_then(|s| s.get::<String>("format").ok())
                .as_deref(),
            Some("DMA_DRM")
        );
        assert_eq!(
            auto_caps
                .structure(1)
                .and_then(|s| s.get::<String>("format").ok())
                .as_deref(),
            Some("BGRA")
        );
        assert_eq!(
            auto_caps
                .structure(2)
                .and_then(|s| s.get::<String>("format").ok())
                .as_deref(),
            Some("BGRA")
        );
        assert!(
            auto_caps
                .features(0)
                .expect("first structure should have caps features")
                .contains(GST_CAPS_FEATURE_MEMORY_DMABUF)
        );
        assert!(
            auto_caps
                .features(1)
                .expect("second structure should have caps features")
                .contains(GST_CAPS_FEATURE_MEMORY_DMABUF)
        );
    }
}
