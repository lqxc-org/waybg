use chrono::{DateTime, Datelike, Local, NaiveTime};
use clap::{Parser, Subcommand};
use freya::prelude::*;
use gst::prelude::*;
use gstreamer as gst;
use serde::Deserialize;
use smithay_client_toolkit::reexports::client::Connection;
use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

type DynError = Box<dyn Error>;

#[derive(Parser, Debug)]
#[command(
    name = "waystream",
    version,
    about = "Wayland video wallpaper controller"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Play one video directly using GStreamer waylandsink.
    Play {
        input: String,
        #[arg(long, default_value_t = false)]
        loop_playback: bool,
    },
    /// Run automatic profile switching loop based on schedule and manual override.
    Auto {
        #[arg(long, default_value = "profiles.toml")]
        config: PathBuf,
    },
    /// Open Freya UI for previewing and selecting profiles.
    Gui {
        #[arg(long, default_value = "profiles.toml")]
        config: PathBuf,
    },
    /// Write a starter profiles config file.
    InitConfig {
        #[arg(long, default_value = "profiles.example.toml")]
        output: PathBuf,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct ProfilesConfig {
    #[serde(default)]
    settings: Settings,
    profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Settings {
    #[serde(default = "default_check_interval_seconds")]
    check_interval_seconds: u64,
    #[serde(default)]
    default_profile: Option<String>,
    #[serde(default)]
    override_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Profile {
    name: String,
    video: String,
    #[serde(default)]
    schedule: Option<ScheduleWindow>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScheduleWindow {
    start: String,
    end: String,
    #[serde(default)]
    weekdays: Vec<u32>,
}

fn default_check_interval_seconds() -> u64 {
    15
}

fn main() -> Result<(), DynError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Play {
            input,
            loop_playback,
        } => play_video(&input, loop_playback),
        Commands::Auto { config } => run_auto_controller(&config),
        Commands::Gui { config } => {
            run_gui(config);
            Ok(())
        }
        Commands::InitConfig { output } => write_example_config(&output),
    }
}

fn play_video(input: &str, loop_playback: bool) -> Result<(), DynError> {
    let uri = to_uri(input)?;
    let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    let _wayland_connection = Connection::connect_to_env().map_err(|error| {
        io::Error::other(format!(
            "failed to connect to Wayland display '{wayland_display}' via SCTK: {error}"
        ))
    })?;

    gst::init()
        .map_err(|error| io::Error::other(format!("failed to initialize GStreamer: {error}")))?;

    let playbin = gst::ElementFactory::make("playbin")
        .name("player")
        .build()
        .map_err(|_| io::Error::other("GStreamer element 'playbin' is unavailable"))?;

    let waylandsink = gst::ElementFactory::make("waylandsink")
        .name("wallpaper_sink")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'waylandsink' is unavailable. Install gst-plugins-bad with Wayland support.",
            )
        })?;

    playbin.set_property("video-sink", &waylandsink);
    playbin.set_property("uri", &uri);

    let bus = playbin
        .bus()
        .ok_or_else(|| io::Error::other("failed to retrieve GStreamer bus"))?;

    playbin.set_state(gst::State::Playing).map_err(|error| {
        io::Error::other(format!("failed to set pipeline to Playing: {error:?}"))
    })?;

    println!("Playing on Wayland display '{wayland_display}': {uri} (loop={loop_playback})");

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

fn run_auto_controller(config_path: &Path) -> Result<(), DynError> {
    let config = ProfilesConfig::load(config_path)?;
    if config.profiles.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "config has no profiles").into());
    }

    let interval_seconds = config.settings.check_interval_seconds.max(1);
    let interval = Duration::from_secs(interval_seconds);
    let override_path = resolve_override_path(config_path, &config);

    println!(
        "Auto mode started with config '{}', override file '{}', interval={}s",
        config_path.display(),
        override_path.display(),
        interval_seconds
    );

    let mut active_profile_name: Option<String> = None;
    let mut running_child: Option<Child> = None;

    loop {
        let manual_override = read_manual_override(&override_path)?;
        let now = Local::now();
        let profile = config
            .pick_profile(manual_override.as_deref(), now)
            .ok_or_else(|| io::Error::other("unable to resolve an active profile"))?;

        if active_profile_name.as_deref() != Some(profile.name.as_str()) {
            if let Some(mut child) = running_child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }

            running_child = Some(spawn_play_process(&profile.video, true)?);
            active_profile_name = Some(profile.name.clone());

            println!(
                "{} active profile -> '{}' ({})",
                now.format("%Y-%m-%d %H:%M:%S"),
                profile.name,
                profile.video
            );
        }

        thread::sleep(interval);
    }
}

fn run_gui(config_path: PathBuf) {
    let launch_config =
        LaunchConfig::new().with_window(WindowConfig::new_app(WallpaperGuiRoot { config_path }));
    launch(launch_config);
}

#[derive(Clone, PartialEq)]
struct WallpaperGuiRoot {
    config_path: PathBuf,
}

impl App for WallpaperGuiRoot {
    fn render(&self) -> impl IntoElement {
        ProfileController {
            config_path: self.config_path.clone(),
        }
    }
}

#[derive(Clone, PartialEq)]
struct ProfileController {
    config_path: PathBuf,
}

#[derive(Clone)]
struct GuiModel {
    config_path: PathBuf,
    override_path: PathBuf,
    profiles: Vec<Profile>,
    selected: usize,
    status: String,
}

impl GuiModel {
    fn load(config_path: PathBuf) -> Self {
        match ProfilesConfig::load(&config_path) {
            Ok(config) => Self {
                override_path: resolve_override_path(&config_path, &config),
                profiles: config.profiles,
                config_path,
                selected: 0,
                status: "Loaded config successfully.".to_string(),
            },
            Err(error) => Self {
                config_path,
                override_path: PathBuf::from("profiles.override"),
                profiles: Vec::new(),
                selected: 0,
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
        let config_path = self.config_path.clone();
        let model = use_state(move || GuiModel::load(config_path.clone()));

        let snapshot = model.read().clone();
        let selected_name = snapshot
            .selected_profile()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "none".to_string());
        let selected_video = snapshot
            .selected_profile()
            .map(|profile| profile.video.clone())
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

        let mut model_prev = model;
        let on_prev = move |_| model_prev.write().prev();

        let mut model_next = model;
        let on_next = move |_| model_next.write().next();

        let mut model_preview = model;
        let on_preview = move |_| {
            let profile = model_preview.read().selected_profile().cloned();
            match profile {
                Some(profile) => match spawn_play_process(&profile.video, false) {
                    Ok(_) => {
                        model_preview.write().status =
                            format!("Started preview for '{}'.", profile.name);
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
            let profile_name = snapshot
                .selected_profile()
                .map(|profile| profile.name.clone());
            match profile_name {
                Some(profile_name) => {
                    let result =
                        write_manual_override(&snapshot.override_path, Some(&profile_name));
                    model_apply.write().status = match result {
                        Ok(()) => format!(
                            "Manual override set to '{}'. Auto mode will pick it up.",
                            profile_name
                        ),
                        Err(error) => format!("Failed to set manual override: {error}"),
                    };
                }
                None => {
                    model_apply.write().status = "No selected profile to apply.".to_string();
                }
            }
        };

        let mut model_auto = model;
        let on_auto = move |_| {
            let override_path = model_auto.read().override_path.clone();
            model_auto.write().status = match write_manual_override(&override_path, None) {
                Ok(()) => "Manual override cleared. Auto schedule is active.".to_string(),
                Err(error) => format!("Failed to clear manual override: {error}"),
            };
        };

        let mut model_reload = model;
        let reload_path = self.config_path.clone();
        let on_reload = move |_| {
            let old_selected_name = model_reload
                .read()
                .selected_profile()
                .map(|profile| profile.name.clone());
            let mut refreshed = GuiModel::load(reload_path.clone());
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

        rect()
            .expanded()
            .padding(16.)
            .spacing(10.)
            .background((17, 20, 28))
            .color((235, 235, 235))
            .child(label().font_size(24.).text("Waystream Freya Profile Controller"))
            .child(label().text(format!("Config: {}", snapshot.config_path.display())))
            .child(label().text(format!(
                "Override file: {}",
                snapshot.override_path.display()
            )))
            .child(label().text(format!("Profiles: {profile_rows}")))
            .child(label().text(format!("Selected: {selected_name}")))
            .child(label().text(format!("Video: {selected_video}")))
            .child(label().text(format!("Schedule: {selected_schedule}")))
            .child(
                rect()
                    .horizontal()
                    .spacing(8.)
                    .child(Button::new().on_press(on_prev).child("Prev"))
                    .child(Button::new().on_press(on_next).child("Next"))
                    .child(Button::new().on_press(on_preview).child("Preview")),
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
                label().text(
                    "Tip: run `waystream auto --config profiles.toml` in another terminal to apply schedule/override continuously.",
                ),
            )
    }
}

impl ProfilesConfig {
    fn load(path: &Path) -> Result<Self, DynError> {
        let raw = fs::read_to_string(path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to read config '{}': {error}", path.display()),
            )
        })?;
        let config: ProfilesConfig = toml::from_str(&raw).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse config '{}': {error}", path.display()),
            )
        })?;
        Ok(config)
    }

    fn pick_profile<'a>(
        &'a self,
        manual_override: Option<&str>,
        now: DateTime<Local>,
    ) -> Option<&'a Profile> {
        if let Some(override_name) = manual_override
            && let Some(profile) = self
                .profiles
                .iter()
                .find(|profile| profile.name == override_name)
        {
            return Some(profile);
        }

        if let Some(profile) = self.profiles.iter().find(|profile| {
            profile
                .schedule
                .as_ref()
                .is_some_and(|schedule| schedule.is_active(now))
        }) {
            return Some(profile);
        }

        if let Some(default_profile) = self.settings.default_profile.as_deref()
            && let Some(profile) = self
                .profiles
                .iter()
                .find(|profile| profile.name == default_profile)
        {
            return Some(profile);
        }

        self.profiles.first()
    }
}

impl ScheduleWindow {
    fn is_active(&self, now: DateTime<Local>) -> bool {
        if !self.weekdays.is_empty() {
            let today = now.weekday().number_from_monday();
            if !self.weekdays.contains(&today) {
                return false;
            }
        }

        let start = parse_hhmm(&self.start);
        let end = parse_hhmm(&self.end);
        let (start, end) = match (start, end) {
            (Some(start), Some(end)) => (start, end),
            _ => return false,
        };
        let current = now.time();

        if start == end {
            true
        } else if start < end {
            current >= start && current < end
        } else {
            current >= start || current < end
        }
    }
}

fn resolve_override_path(config_path: &Path, config: &ProfilesConfig) -> PathBuf {
    match config.settings.override_file.as_deref() {
        Some(path) => {
            let custom = PathBuf::from(path);
            if custom.is_absolute() {
                custom
            } else {
                config_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(custom)
            }
        }
        None => config_path.with_extension("override"),
    }
}

fn parse_hhmm(input: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(input, "%H:%M").ok()
}

fn read_manual_override(path: &Path) -> Result<Option<String>, io::Error> {
    match fs::read_to_string(path) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_manual_override(path: &Path, profile: Option<&str>) -> Result<(), io::Error> {
    if let Some(profile) = profile {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, format!("{profile}\n"))?;
    } else {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn spawn_play_process(input: &str, loop_playback: bool) -> Result<Child, io::Error> {
    let exe = env::current_exe()?;
    let mut command = Command::new(exe);
    command.arg("play").arg(input);
    if loop_playback {
        command.arg("--loop-playback");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
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

fn write_example_config(output: &Path) -> Result<(), DynError> {
    const TEMPLATE: &str = r#"[settings]
check_interval_seconds = 15
default_profile = "day"
# override_file = "profiles.override"

[[profiles]]
name = "day"
video = "/absolute/path/to/day.mp4"
[profiles.schedule]
start = "08:00"
end = "18:00"
weekdays = [1, 2, 3, 4, 5]

[[profiles]]
name = "night"
video = "/absolute/path/to/night.mp4"
[profiles.schedule]
start = "18:00"
end = "08:00"

[[profiles]]
name = "fallback"
video = "/absolute/path/to/fallback.mp4"
"#;

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, TEMPLATE)?;
    println!("Wrote example config to '{}'.", output.display());
    Ok(())
}
