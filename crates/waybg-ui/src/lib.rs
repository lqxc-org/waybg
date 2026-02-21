use freya::{
    prelude::*,
    winit::window::{Icon, WindowAttributes},
};
use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
use waybg_core::{
    FsOverrideStore, OverrideStore, Profile, ProfilesConfig, ensure_config_exists,
    resolve_override_path,
};

const APP_NAME: &str = "Waybg";
const APP_ID: &str = "org.lqxc.waybg";
const APP_ICON_URL: &str = "https://collects-cdn-test.lqxclqxc.com/public/collects/101-oldchicken-stickers/items/019c173b-b24a-7f1a-ad35-d1f62ae38b72";

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

pub fn run_gui(options: GuiRuntimeOptions) {
    let mut window = WindowConfig::new_app(WallpaperGuiRoot { options })
        .with_title(APP_NAME)
        .with_size(1024.0, 720.0)
        .with_window_attributes(|attributes, _active_event_loop| set_app_id(attributes));

    if let Some(icon) = load_remote_icon() {
        window = window.with_icon(icon);
    }

    let launch_config = LaunchConfig::new().with_window(window);
    launch(launch_config);
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
                    override_path: PathBuf::from("profiles.override"),
                    player_executable,
                    player_prefix_args,
                    profiles: Vec::new(),
                    selected: 0,
                    status: format!("Config bootstrap failed: {error}"),
                };
            }
        };

        match ProfilesConfig::load(&config_path) {
            Ok(config) => Self {
                override_path: resolve_override_path(&config_path, &config),
                profiles: config.profiles,
                config_path,
                player_executable,
                player_prefix_args,
                selected: 0,
                status: if generated {
                    "Generated missing config and loaded it successfully.".to_string()
                } else {
                    "Loaded config successfully.".to_string()
                },
            },
            Err(error) => Self {
                config_path,
                override_path: PathBuf::from("profiles.override"),
                player_executable,
                player_prefix_args,
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
        let options = self.options.clone();
        let model = use_state(move || GuiModel::load(options.clone()));

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
            let snapshot = model_preview.read().clone();
            let profile = snapshot.selected_profile().cloned();
            match profile {
                Some(profile) => match spawn_play_process(
                    &snapshot.player_executable,
                    &snapshot.player_prefix_args,
                    &profile.video,
                    false,
                ) {
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
                    let store = FsOverrideStore;
                    let result =
                        store.write_manual_override(&snapshot.override_path, Some(&profile_name));
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
            .child(label().font_size(24.).text("Waybg Freya Profile Controller"))
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
                    "Tip: run `waybg auto --config profiles.toml` in another terminal to apply schedule/override continuously.",
                ),
            )
    }
}

fn spawn_play_process(
    executable: &Path,
    prefix_args: &[String],
    input: &str,
    loop_playback: bool,
) -> Result<Child, io::Error> {
    let mut command = Command::new(executable);
    command.args(prefix_args).arg(input);
    if loop_playback {
        command.arg("--loop-playback");
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}
