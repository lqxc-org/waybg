use bytes::Bytes;
use freya::{
    prelude::*,
    winit::window::{Icon, WindowAttributes},
};
use notify_rust::Notification;
use plotters::prelude::{
    ChartBuilder, IntoDrawingArea, IntoFont, LineSeries, RGBColor, SVGBackend, WHITE,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use waybg_core::{
    DynError, FsOverrideStore, OverrideStore, Profile, ProfilesConfig, RenderTarget,
    SystemTimeProvider, TimeProvider, default_override_path, ensure_config_exists,
    resolve_override_path, summarize_render_targets,
};
use wayland_core::PlaybackMetricsSnapshot;

const APP_NAME: &str = "Waybg";
const APP_ID: &str = "org.lqxc.waybg";
const APP_ICON_URL: &str = "https://collects-cdn-test.lqxclqxc.com/public/collects/101-oldchicken-stickers/items/019c173b-b24a-7f1a-ad35-d1f62ae38b72";
const METRICS_CHART_WIDTH: u32 = 960;
const METRICS_CHART_HEIGHT: u32 = 240;
const METRICS_REFRESH_INTERVAL_MS: u64 = 250;
const METRICS_REFRESH_INTERVAL_MIN_MS: u64 = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct GuiRuntimeOptions {
    pub config_path: PathBuf,
    pub player_executable: PathBuf,
    pub player_prefix_args: Vec<String>,
}

impl GuiRuntimeOptions {
    pub fn new(
        config_path: PathBuf,
        player_executable: PathBuf,
        player_prefix_args: Vec<String>,
    ) -> Self {
        Self {
            config_path,
            player_executable,
            player_prefix_args,
        }
    }
}

pub fn run_gui(options: GuiRuntimeOptions) -> Result<(), DynError> {
    validate_startup_or_notify(&options)?;

    let mut window = WindowConfig::new_app(WallpaperGuiRoot { options })
        .with_title(APP_NAME)
        .with_size(1024.0, 720.0)
        .with_window_attributes(|attributes, _active_event_loop| set_app_id(attributes));

    if let Some(icon) = load_remote_icon() {
        window = window.with_icon(icon);
    }

    let launch_config = LaunchConfig::new().with_window(window);
    launch(launch_config);
    Ok(())
}

fn validate_startup_or_notify(options: &GuiRuntimeOptions) -> Result<(), DynError> {
    let startup_result = (|| -> Result<(), DynError> {
        if ensure_config_exists(&options.config_path)? {
            println!(
                "Config '{}' did not exist; generated an example config.",
                options.config_path.display()
            );
        }
        let config = ProfilesConfig::load(&options.config_path)?;
        let override_path = resolve_override_path(&options.config_path, &config)?;
        apply_startup_profile(options, &config, &override_path)?;
        Ok(())
    })();

    if let Err(error) = startup_result {
        notify_startup_error(&options.config_path, &error.to_string());
        return Err(error);
    }

    Ok(())
}

fn apply_startup_profile(
    options: &GuiRuntimeOptions,
    config: &ProfilesConfig,
    override_path: &Path,
) -> Result<(), DynError> {
    let profile = resolve_active_profile(config, override_path)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "config has no profiles"))?;

    let count = spawn_targets(
        &options.player_executable,
        &options.player_prefix_args,
        profile,
        true,
        config.settings.mute,
        override_path,
        true,
    )?;

    println!(
        "Applied startup profile '{}' on {count} output(s), audio {}.",
        profile.name,
        if config.settings.mute {
            "muted"
        } else {
            "unmuted"
        }
    );

    Ok(())
}

fn resolve_active_profile<'a>(
    config: &'a ProfilesConfig,
    override_path: &Path,
) -> Result<Option<&'a Profile>, io::Error> {
    let store = FsOverrideStore;
    let manual_override = store.read_manual_override(override_path)?;
    let clock = SystemTimeProvider;
    Ok(config.pick_profile(manual_override.as_deref(), clock.now()))
}

fn resolve_active_profile_index(
    config: &ProfilesConfig,
    override_path: &Path,
) -> Result<usize, io::Error> {
    if config.profiles.is_empty() {
        return Ok(0);
    }
    if let Some(profile) = resolve_active_profile(config, override_path)?
        && let Some(index) = config
            .profiles
            .iter()
            .position(|candidate| candidate.name == profile.name)
    {
        return Ok(index);
    }
    Ok(0)
}

fn sanitize_metrics_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

fn metrics_file_for_target(
    override_path: &Path,
    profile_name: &str,
    output: Option<&str>,
    index: usize,
) -> PathBuf {
    let profile = sanitize_metrics_component(profile_name);
    let output = sanitize_metrics_component(output.unwrap_or("all"));
    override_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("metrics")
        .join(format!("{profile}--{output}--{index}.json"))
}

fn primary_target_with_metrics_path(
    profile: &Profile,
    override_path: &Path,
) -> Option<(RenderTarget, PathBuf)> {
    let target = profile.render_targets().into_iter().next()?;
    let metrics_path =
        metrics_file_for_target(override_path, &profile.name, target.output.as_deref(), 0);
    Some((target, metrics_path))
}

fn load_metrics_snapshot(path: &Path) -> Result<PlaybackMetricsSnapshot, io::Error> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw)
        .map_err(|error| io::Error::other(format!("failed to parse metrics JSON: {error}")))
}

fn render_fps_chart_svg(metrics: &PlaybackMetricsSnapshot) -> Result<String, DynError> {
    let mut series = metrics.recent_fps.clone();
    if series.is_empty() {
        series.push(0.0);
    }
    if series.len() == 1 {
        series.push(series[0]);
    }

    let y_max = (series.iter().copied().fold(1.0, f64::max) * 1.2)
        .max(30.0)
        .ceil();
    let x_max = series.len().saturating_sub(1) as i32;
    let x_range_end = (x_max + 1).max(1);

    let mut svg = String::new();
    {
        let root = SVGBackend::with_string(&mut svg, (METRICS_CHART_WIDTH, METRICS_CHART_HEIGHT))
            .into_drawing_area();
        root.fill(&RGBColor(18, 22, 30))?;

        let mut chart = ChartBuilder::on(&root)
            .margin(12)
            .x_label_area_size(22)
            .y_label_area_size(42)
            .caption(
                "FPS (recent samples)",
                ("sans-serif", 16).into_font().color(&WHITE),
            )
            .build_cartesian_2d(0i32..x_range_end, 0f64..y_max)?;

        chart
            .configure_mesh()
            .x_labels(6)
            .y_labels(6)
            .label_style(
                ("sans-serif", 10)
                    .into_font()
                    .color(&RGBColor(220, 220, 220)),
            )
            .axis_style(RGBColor(150, 150, 150))
            .light_line_style(RGBColor(42, 48, 60))
            .draw()?;

        chart.draw_series(LineSeries::new(
            series
                .iter()
                .enumerate()
                .map(|(index, fps)| (index as i32, *fps)),
            &RGBColor(73, 184, 247),
        ))?;

        for (value, color) in [
            (metrics.avg_fps, RGBColor(120, 215, 120)),
            (metrics.low95_fps, RGBColor(248, 196, 90)),
            (metrics.low99_fps, RGBColor(247, 124, 124)),
        ] {
            chart.draw_series(LineSeries::new([(0i32, value), (x_max, value)], &color))?;
        }

        root.present()?;
    }
    Ok(svg)
}

fn format_fps(value: f64) -> String {
    format!("{value:.1}")
}

fn empty_metrics_svg() -> Bytes {
    Bytes::from_static(
        br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 960 240">
<rect x="0" y="0" width="960" height="240" fill="#12161e"/>
<text x="24" y="52" fill="#d0d6e2" font-size="24" font-family="sans-serif">No FPS metrics yet</text>
<text x="24" y="86" fill="#8f9aad" font-size="16" font-family="sans-serif">Press Preview/Apply, then Refresh Metrics.</text>
</svg>"##,
    )
}

fn notify_startup_error(config_path: &Path, error: &str) {
    let message = format!("Config '{}': {error}", config_path.display());
    let _ = Notification::new()
        .appname(APP_NAME)
        .summary("Waybg startup failed")
        .body(&message)
        .show();
}

fn set_app_id(attributes: WindowAttributes) -> WindowAttributes {
    #[cfg(target_os = "linux")]
    {
        use freya::winit::platform::wayland::WindowAttributesExtWayland;

        attributes.with_name(APP_ID, APP_NAME)
    }

    #[cfg(not(target_os = "linux"))]
    {
        attributes
    }
}

fn load_remote_icon() -> Option<Icon> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "4",
            APP_ICON_URL,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    std::panic::catch_unwind(|| LaunchConfig::window_icon(&output.stdout)).ok()
}

#[derive(Clone, PartialEq)]
struct WallpaperGuiRoot {
    options: GuiRuntimeOptions,
}

impl App for WallpaperGuiRoot {
    fn render(&self) -> impl IntoElement {
        ProfileController {
            options: self.options.clone(),
        }
    }
}

#[derive(Clone, PartialEq)]
struct ProfileController {
    options: GuiRuntimeOptions,
}

#[derive(Clone)]
struct GuiModel {
    config_path: PathBuf,
    override_path: PathBuf,
    player_executable: PathBuf,
    player_prefix_args: Vec<String>,
    profiles: Vec<Profile>,
    selected: usize,
    muted: bool,
    metrics_capture_enabled: bool,
    metrics_auto_refresh: bool,
    metrics_refresh_interval_ms: u64,
    metrics_refresh_nonce: u64,
    status: String,
}

impl GuiModel {
    fn load(options: GuiRuntimeOptions) -> Self {
        let GuiRuntimeOptions {
            config_path,
            player_executable,
            player_prefix_args,
        } = options;
        let generated = match ensure_config_exists(&config_path) {
            Ok(generated) => generated,
            Err(error) => {
                return Self {
                    config_path,
                    override_path: default_override_path()
                        .unwrap_or_else(|_| PathBuf::from("<unresolved>")),
                    player_executable,
                    player_prefix_args,
                    profiles: Vec::new(),
                    selected: 0,
                    muted: false,
                    metrics_capture_enabled: true,
                    metrics_auto_refresh: true,
                    metrics_refresh_interval_ms: METRICS_REFRESH_INTERVAL_MS,
                    metrics_refresh_nonce: 0,
                    status: format!("Config bootstrap failed: {error}"),
                };
            }
        };

        match ProfilesConfig::load(&config_path) {
            Ok(config) => match resolve_override_path(&config_path, &config) {
                Ok(override_path) => {
                    let selected =
                        resolve_active_profile_index(&config, &override_path).unwrap_or(0);
                    Self {
                        override_path,
                        profiles: config.profiles,
                        config_path,
                        player_executable,
                        player_prefix_args,
                        selected,
                        muted: config.settings.mute,
                        metrics_capture_enabled: true,
                        metrics_auto_refresh: true,
                        metrics_refresh_interval_ms: METRICS_REFRESH_INTERVAL_MS,
                        metrics_refresh_nonce: 0,
                        status: if generated {
                            "Generated missing config and loaded it successfully.".to_string()
                        } else {
                            "Loaded config successfully.".to_string()
                        },
                    }
                }
                Err(error) => Self {
                    config_path,
                    override_path: PathBuf::from("<unresolved>"),
                    player_executable,
                    player_prefix_args,
                    profiles: config.profiles,
                    selected: 0,
                    muted: config.settings.mute,
                    metrics_capture_enabled: true,
                    metrics_auto_refresh: true,
                    metrics_refresh_interval_ms: METRICS_REFRESH_INTERVAL_MS,
                    metrics_refresh_nonce: 0,
                    status: format!("Config loaded, but override path resolution failed: {error}"),
                },
            },
            Err(error) => Self {
                config_path,
                override_path: default_override_path()
                    .unwrap_or_else(|_| PathBuf::from("<unresolved>")),
                player_executable,
                player_prefix_args,
                profiles: Vec::new(),
                selected: 0,
                muted: false,
                metrics_capture_enabled: true,
                metrics_auto_refresh: true,
                metrics_refresh_interval_ms: METRICS_REFRESH_INTERVAL_MS,
                metrics_refresh_nonce: 0,
                status: format!("Config load failed: {error}"),
            },
        }
    }

    fn selected_profile(&self) -> Option<&Profile> {
        self.profiles.get(self.selected)
    }

    fn next(&mut self) {
        if self.profiles.is_empty() {
            self.status = "No profiles available.".to_string();
            return;
        }
        self.selected = (self.selected + 1) % self.profiles.len();
        if let Some(profile) = self.selected_profile() {
            self.status = format!("Selected profile '{}'.", profile.name);
        }
    }

    fn prev(&mut self) {
        if self.profiles.is_empty() {
            self.status = "No profiles available.".to_string();
            return;
        }
        self.selected = if self.selected == 0 {
            self.profiles.len() - 1
        } else {
            self.selected - 1
        };
        if let Some(profile) = self.selected_profile() {
            self.status = format!("Selected profile '{}'.", profile.name);
        }
    }
}

impl Component for ProfileController {
    fn render(&self) -> impl IntoElement {
        let options = self.options.clone();
        let model = use_state(move || GuiModel::load(options.clone()));
        let mut model_poller = model;
        let _metrics_poller = use_hook(move || {
            spawn(async move {
                let mut ticker = RenderingTicker::get();
                let mut last_refresh = Instant::now();
                loop {
                    ticker.tick().await;
                    let (auto_refresh, interval_ms) = {
                        let state = model_poller.read();
                        (
                            state.metrics_auto_refresh,
                            state
                                .metrics_refresh_interval_ms
                                .max(METRICS_REFRESH_INTERVAL_MIN_MS),
                        )
                    };
                    if !auto_refresh {
                        continue;
                    }
                    let interval = Duration::from_millis(interval_ms);
                    if last_refresh.elapsed() < interval {
                        continue;
                    }
                    last_refresh = Instant::now();
                    let next_nonce = {
                        let state = model_poller.read();
                        state.metrics_refresh_nonce.wrapping_add(1)
                    };
                    model_poller.write().metrics_refresh_nonce = next_nonce;
                }
            })
        });

        let snapshot = model.read().clone();
        let selected_name = snapshot
            .selected_profile()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "none".to_string());
        let selected_video = snapshot
            .selected_profile()
            .map(|profile| summarize_render_targets(&profile.render_targets()))
            .unwrap_or_else(|| "n/a".to_string());
        let selected_schedule = snapshot
            .selected_profile()
            .and_then(|profile| profile.schedule.as_ref())
            .map(|schedule| {
                format!(
                    "{}-{}{}",
                    schedule.start,
                    schedule.end,
                    if schedule.weekdays.is_empty() {
                        "".to_string()
                    } else {
                        format!(" weekdays={:?}", schedule.weekdays)
                    }
                )
            })
            .unwrap_or_else(|| "always/fallback".to_string());
        let audio_status = if snapshot.muted { "muted" } else { "unmuted" };
        let profile_rows = if snapshot.profiles.is_empty() {
            "No profiles loaded.".to_string()
        } else {
            snapshot
                .profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| {
                    if index == snapshot.selected {
                        format!("> {}", profile.name)
                    } else {
                        format!("  {}", profile.name)
                    }
                })
                .collect::<Vec<_>>()
                .join("   ")
        };
        let (metrics_target_label, metrics_path_text, metrics_snapshot, metrics_error_text) =
            if !snapshot.metrics_capture_enabled {
                (
                    "n/a".to_string(),
                    "<disabled>".to_string(),
                    None,
                    Some("Metrics capture is disabled.".to_string()),
                )
            } else {
                match snapshot.selected_profile().and_then(|profile| {
                    primary_target_with_metrics_path(profile, &snapshot.override_path)
                }) {
                    Some((target, path)) => {
                        let target_label =
                            target.output.unwrap_or_else(|| "all outputs".to_string());
                        let path_text = path.display().to_string();
                        match load_metrics_snapshot(&path) {
                            Ok(metrics) => (target_label, path_text, Some(metrics), None),
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                                (target_label, path_text, None, None)
                            }
                            Err(error) => (
                                target_label,
                                path_text,
                                None,
                                Some(format!("Metrics read failed: {error}")),
                            ),
                        }
                    }
                    None => (
                        "n/a".to_string(),
                        "<no target>".to_string(),
                        None,
                        Some("No target selected.".to_string()),
                    ),
                }
            };
        let metrics_summary = metrics_snapshot
            .as_ref()
            .map(|metrics| {
                format!(
                    "avg={}  low95={}  low99={}  min={}  max={}  last={}  samples={}",
                    format_fps(metrics.avg_fps),
                    format_fps(metrics.low95_fps),
                    format_fps(metrics.low99_fps),
                    format_fps(metrics.min_fps),
                    format_fps(metrics.max_fps),
                    format_fps(metrics.last_fps),
                    metrics.sample_count
                )
            })
            .unwrap_or_else(|| "No FPS samples yet.".to_string());
        let metrics_notes = metrics_snapshot
            .as_ref()
            .and_then(|metrics| metrics.notes.as_deref())
            .unwrap_or("none")
            .to_string();
        let hardware_decoders = metrics_snapshot
            .as_ref()
            .map(|metrics| {
                if metrics.hardware_decoders.is_empty() {
                    "none detected".to_string()
                } else {
                    metrics.hardware_decoders.join(", ")
                }
            })
            .unwrap_or_else(|| "unknown".to_string());
        let metrics_svg_bytes = metrics_snapshot
            .as_ref()
            .and_then(|metrics| render_fps_chart_svg(metrics).ok())
            .map(|svg| Bytes::from(svg.into_bytes()))
            .unwrap_or_else(empty_metrics_svg);

        let mut model_prev = model;
        let on_prev = move |_| model_prev.write().prev();

        let mut model_next = model;
        let on_next = move |_| model_next.write().next();

        let mut model_preview = model;
        let on_preview = move |_| {
            let snapshot = model_preview.read().clone();
            let profile = snapshot.selected_profile().cloned();
            match profile {
                Some(profile) => match spawn_targets(
                    &snapshot.player_executable,
                    &snapshot.player_prefix_args,
                    &profile,
                    false,
                    snapshot.muted,
                    &snapshot.override_path,
                    snapshot.metrics_capture_enabled,
                ) {
                    Ok(count) => {
                        let audio_status = if snapshot.muted { "muted" } else { "unmuted" };
                        model_preview.write().status = format!(
                            "Started preview for '{}' on {count} output(s), audio {}.",
                            profile.name, audio_status
                        );
                    }
                    Err(error) => {
                        model_preview.write().status = format!("Preview failed: {error}");
                    }
                },
                None => {
                    model_preview.write().status = "No selected profile to preview.".to_string();
                }
            }
        };

        let mut model_apply = model;
        let on_apply = move |_| {
            let snapshot = model_apply.read().clone();
            let selected_profile = snapshot.selected_profile().cloned();
            match selected_profile {
                Some(profile) => {
                    match spawn_targets(
                        &snapshot.player_executable,
                        &snapshot.player_prefix_args,
                        &profile,
                        true,
                        snapshot.muted,
                        &snapshot.override_path,
                        snapshot.metrics_capture_enabled,
                    ) {
                        Ok(count) => {
                            let store = FsOverrideStore;
                            let profile_name = profile.name.clone();
                            let result = store.write_manual_override(
                                &snapshot.override_path,
                                Some(&profile_name),
                            );
                            model_apply.write().status = match result {
                                Ok(()) => format!(
                                    "Applied '{}' on {count} output(s) and set manual override.",
                                    profile_name
                                ),
                                Err(error) => format!(
                                    "Applied '{}' on {count} output(s), but failed to persist manual override: {error}",
                                    profile_name
                                ),
                            };
                        }
                        Err(error) => {
                            model_apply.write().status =
                                format!("Failed to apply selected profile: {error}");
                        }
                    }
                }
                None => {
                    model_apply.write().status = "No selected profile to apply.".to_string();
                }
            }
        };

        let mut model_auto = model;
        let on_auto = move |_| {
            let override_path = model_auto.read().override_path.clone();
            let store = FsOverrideStore;
            model_auto.write().status = match store.write_manual_override(&override_path, None) {
                Ok(()) => "Manual override cleared. Auto schedule is active.".to_string(),
                Err(error) => format!("Failed to clear manual override: {error}"),
            };
        };

        let mut model_reload = model;
        let on_reload = move |_| {
            let snapshot = model_reload.read().clone();
            let old_selected_name = snapshot
                .selected_profile()
                .map(|profile| profile.name.clone());
            let mut refreshed = GuiModel::load(GuiRuntimeOptions {
                config_path: snapshot.config_path.clone(),
                player_executable: snapshot.player_executable.clone(),
                player_prefix_args: snapshot.player_prefix_args.clone(),
            });
            refreshed.metrics_capture_enabled = snapshot.metrics_capture_enabled;
            refreshed.metrics_auto_refresh = snapshot.metrics_auto_refresh;
            refreshed.metrics_refresh_interval_ms = snapshot.metrics_refresh_interval_ms;
            refreshed.metrics_refresh_nonce = snapshot.metrics_refresh_nonce;
            if let Some(selected_name) = old_selected_name
                && let Some(index) = refreshed
                    .profiles
                    .iter()
                    .position(|profile| profile.name == selected_name)
            {
                refreshed.selected = index;
            }
            *model_reload.write() = refreshed;
        };

        let mut model_audio = model;
        let on_toggle_audio = move |_| {
            let snapshot = model_audio.read().clone();
            let next_muted = !snapshot.muted;
            let status = match update_config_mute(&snapshot.config_path, next_muted) {
                Ok(()) => {
                    let mut state = model_audio.write();
                    state.muted = next_muted;
                    if next_muted {
                        "Audio muted in config. Auto mode applies this on next tick.".to_string()
                    } else {
                        "Audio unmuted in config. Auto mode applies this on next tick.".to_string()
                    }
                }
                Err(error) => format!("Failed to update mute setting: {error}"),
            };
            model_audio.write().status = status;
        };

        let mut model_capture = model;
        let on_toggle_metrics_capture = move |_| {
            let snapshot = model_capture.read().clone();
            let next_capture_enabled = !snapshot.metrics_capture_enabled;
            let status = if next_capture_enabled {
                "Metrics capture enabled for next preview/apply run.".to_string()
            } else {
                "Metrics capture disabled. Playback keeps running, but no new metrics files are written.".to_string()
            };
            let mut state = model_capture.write();
            state.metrics_capture_enabled = next_capture_enabled;
            state.status = status;
        };

        let mut model_live_metrics = model;
        let on_toggle_live_metrics = move |_| {
            let snapshot = model_live_metrics.read().clone();
            let next_live = !snapshot.metrics_auto_refresh;
            let status = if next_live {
                format!(
                    "Live metrics refresh enabled ({}ms).",
                    snapshot.metrics_refresh_interval_ms
                )
            } else {
                "Live metrics refresh paused. Use Trigger Snapshot to pull metrics manually."
                    .to_string()
            };
            let mut state = model_live_metrics.write();
            state.metrics_auto_refresh = next_live;
            state.status = status;
        };

        let mut model_metrics = model;
        let on_refresh_metrics = move |_| {
            let snapshot = model_metrics.read().clone();
            let next_nonce = snapshot.metrics_refresh_nonce.wrapping_add(1);
            let status = match snapshot.selected_profile() {
                Some(profile) => format!("Refreshed metrics for '{}'.", profile.name),
                None => "No profile selected for metrics refresh.".to_string(),
            };
            let mut state = model_metrics.write();
            state.metrics_refresh_nonce = next_nonce;
            state.status = status;
        };

        rect()
            .expanded()
            .padding(16.)
            .spacing(10.)
            .background((17, 20, 28))
            .color((235, 235, 235))
            .child(label().font_size(24.).text("Waybg Freya Profile Controller"))
            .child(label().text(format!("Config: {}", snapshot.config_path.display())))
            .child(label().text(format!(
                "Override file: {}",
                snapshot.override_path.display()
            )))
            .child(label().text(format!("Profiles: {profile_rows}")))
            .child(label().text(format!("Selected: {selected_name}")))
            .child(label().text(format!("Video: {selected_video}")))
            .child(label().text(format!("Audio: {audio_status}")))
            .child(label().text(format!("Schedule: {selected_schedule}")))
            .child(label().text(format!("Metrics target: {metrics_target_label}")))
            .child(label().text(format!("Metrics file: {metrics_path_text}")))
            .child(label().text(format!("FPS summary: {metrics_summary}")))
            .child(label().text(format!("Hardware decoders: {hardware_decoders}")))
            .child(label().text(format!("Metrics notes: {metrics_notes}")))
            .child(label().text(format!(
                "Metrics capture: {}",
                if snapshot.metrics_capture_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            )))
            .child(label().text(format!(
                "Refresh mode: {} (interval={}ms, tick={})",
                if snapshot.metrics_auto_refresh {
                    "live"
                } else {
                    "manual"
                },
                snapshot.metrics_refresh_interval_ms,
                snapshot.metrics_refresh_nonce
            )))
            .child(label().text(format!(
                "Metrics status: {}",
                metrics_error_text
                    .clone()
                    .unwrap_or_else(|| "ok".to_string())
            )))
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::px(METRICS_CHART_HEIGHT as f32))
                    .child(
                        svg(metrics_svg_bytes)
                            .width(Size::fill())
                            .height(Size::fill()),
                    ),
            )
            .child(
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(Button::new().on_press(on_prev).child("Prev"))
                    .child(Button::new().on_press(on_next).child("Next"))
                    .child(Button::new().on_press(on_preview).child("Preview"))
                    .child(Button::new().on_press(on_toggle_audio).child(
                        if snapshot.muted { "Unmute" } else { "Mute" }
                    )),
            )
            .child(
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(Button::new().on_press(on_toggle_metrics_capture).child(
                        if snapshot.metrics_capture_enabled {
                            "Disable Capture"
                        } else {
                            "Enable Capture"
                        }
                    ))
                    .child(Button::new().on_press(on_toggle_live_metrics).child(
                        if snapshot.metrics_auto_refresh {
                            "Pause Live"
                        } else {
                            "Resume Live"
                        }
                    ))
                    .child(Button::new().on_press(on_refresh_metrics).child("Trigger Snapshot")),
            )
            .child(
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(Button::new().on_press(on_apply).child("Apply Manual"))
                    .child(Button::new().on_press(on_auto).child("Enable Auto"))
                    .child(Button::new().on_press(on_reload).child("Reload Config")),
            )
            .child(label().text(format!("Status: {}", snapshot.status)))
            .child(
                label().text(format!(
                    "Tip: run `waybg auto --config {}` in another terminal to apply schedule/override continuously.",
                    snapshot.config_path.display()
                )),
            )
    }
}

fn spawn_play_process(
    executable: &Path,
    prefix_args: &[String],
    input: &str,
    loop_playback: bool,
    output: Option<&str>,
    mute: bool,
    metrics_file: Option<&Path>,
) -> Result<Child, io::Error> {
    let mut command = Command::new(executable);
    command.args(prefix_args).arg(input);
    if loop_playback {
        command.arg("--loop-playback");
    }
    if let Some(output) = output {
        command.args(["--output", output]);
    }
    if mute {
        command.arg("--mute");
    }
    if let Some(metrics_file) = metrics_file {
        command.arg("--metrics-file").arg(metrics_file);
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

fn spawn_targets(
    executable: &Path,
    prefix_args: &[String],
    profile: &Profile,
    loop_playback: bool,
    mute: bool,
    override_path: &Path,
    capture_metrics: bool,
) -> Result<usize, io::Error> {
    let targets = profile.render_targets();
    if targets.is_empty() {
        return Err(io::Error::other("no render targets found"));
    }

    let mut started = 0usize;
    for (index, target) in targets.into_iter().enumerate() {
        let metrics_file = if capture_metrics {
            Some(metrics_file_for_target(
                override_path,
                &profile.name,
                target.output.as_deref(),
                index,
            ))
        } else {
            None
        };
        spawn_play_process(
            executable,
            prefix_args,
            &target.video,
            loop_playback,
            target.output.as_deref(),
            mute,
            metrics_file.as_deref(),
        )?;
        started += 1;
    }

    Ok(started)
}

fn update_config_mute(config_path: &Path, mute: bool) -> Result<(), DynError> {
    let mut config = ProfilesConfig::load(config_path)?;
    config.settings.mute = mute;
    let encoded = toml::to_string_pretty(&config)
        .map_err(|error| io::Error::other(format!("failed to encode config: {error}")))?;
    fs::write(config_path, encoded)?;
    Ok(())
}
