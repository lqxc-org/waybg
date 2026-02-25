use chrono::{Local, TimeZone};
use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    rc::Rc,
};
use tempfile::TempDir;
use waybg_core::{
    APP_DIR_NAME, AutoController, DEFAULT_OVERRIDE_FILENAME, OverrideStore, PlaybackLauncher,
    PlaybackProcess, Profile, ProfileOutput, ProfilesConfig, ScheduleWindow, Settings,
    TimeProvider, ensure_config_exists, read_manual_override, resolve_override_path,
    write_manual_override,
};

type SpawnLog = Rc<RefCell<Vec<(String, Option<String>)>>>;

#[test]
fn manual_override_round_trip_works_on_filesystem() -> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let path = temp.path().join("profiles.override");

    assert_eq!(read_manual_override(&path)?, None);

    write_manual_override(&path, Some("night"))?;
    assert_eq!(read_manual_override(&path)?, Some("night".to_string()));

    write_manual_override(&path, None)?;
    assert_eq!(read_manual_override(&path)?, None);

    Ok(())
}

#[test]
fn resolve_override_path_uses_relative_path_from_config_dir()
-> Result<(), Box<dyn std::error::Error>> {
    let config_path = Path::new("/tmp/waybg/profiles.toml");
    let config = ProfilesConfig {
        settings: Settings {
            check_interval_seconds: 15,
            default_profile: None,
            override_file: Some("state/current.override".to_string()),
            mute: false,
        },
        profiles: vec![Profile {
            name: "fallback".to_string(),
            video: "fallback.mp4".to_string(),
            outputs: Vec::new(),
            schedule: None,
        }],
    };

    let resolved = resolve_override_path(config_path, &config)?;
    assert_eq!(resolved, PathBuf::from("/tmp/waybg/state/current.override"));
    Ok(())
}

#[test]
fn resolve_override_path_defaults_to_xdg_state_path() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = Path::new("/tmp/waybg/profiles.toml");
    let config = ProfilesConfig {
        settings: Settings {
            check_interval_seconds: 15,
            default_profile: None,
            override_file: None,
            mute: false,
        },
        profiles: vec![Profile {
            name: "fallback".to_string(),
            video: "fallback.mp4".to_string(),
            outputs: Vec::new(),
            schedule: None,
        }],
    };

    match std::env::var_os("XDG_STATE_HOME") {
        Some(value) if !value.is_empty() => {
            let state_home = PathBuf::from(value);
            if state_home.is_absolute() {
                let resolved = resolve_override_path(config_path, &config)?;
                assert_eq!(
                    resolved,
                    state_home
                        .join(APP_DIR_NAME)
                        .join(DEFAULT_OVERRIDE_FILENAME)
                );
            } else {
                let error = resolve_override_path(config_path, &config)
                    .expect_err("relative XDG_STATE_HOME should fail")
                    .to_string();
                assert!(error.contains("XDG_STATE_HOME"));
            }
        }
        _ => {
            let home = std::env::var_os("HOME");
            if let Some(home) = home {
                let home = PathBuf::from(home);
                if home.is_absolute() {
                    let resolved = resolve_override_path(config_path, &config)?;
                    assert_eq!(
                        resolved,
                        home.join(".local/state")
                            .join(APP_DIR_NAME)
                            .join(DEFAULT_OVERRIDE_FILENAME)
                    );
                } else {
                    let error = resolve_override_path(config_path, &config)
                        .expect_err("relative HOME should fail")
                        .to_string();
                    assert!(error.contains("HOME"));
                }
            } else {
                let error = resolve_override_path(config_path, &config)
                    .expect_err("missing HOME should fail")
                    .to_string();
                assert!(error.contains("HOME"));
            }
        }
    }
    Ok(())
}

#[test]
fn schedule_window_supports_overnight_range() {
    let schedule = ScheduleWindow {
        start: "18:00".to_string(),
        end: "08:00".to_string(),
        weekdays: Vec::new(),
    };

    let night = Local
        .with_ymd_and_hms(2026, 2, 18, 23, 0, 0)
        .single()
        .expect("valid fixed timestamp");
    let morning = Local
        .with_ymd_and_hms(2026, 2, 18, 7, 0, 0)
        .single()
        .expect("valid fixed timestamp");
    let noon = Local
        .with_ymd_and_hms(2026, 2, 18, 12, 0, 0)
        .single()
        .expect("valid fixed timestamp");

    assert!(schedule.is_active(night));
    assert!(schedule.is_active(morning));
    assert!(!schedule.is_active(noon));
}

#[test]
fn profile_video_defaults_to_blank_when_field_is_omitted() -> Result<(), Box<dyn std::error::Error>>
{
    let raw = r#"
[[profiles]]
name = "fallback"
"#;
    let config: ProfilesConfig = toml::from_str(raw)?;
    assert_eq!(config.profiles.len(), 1);
    assert_eq!(config.profiles[0].video, "blank://");
    assert!(config.profiles[0].outputs.is_empty());
    Ok(())
}

#[test]
fn profile_outputs_define_render_targets() -> Result<(), Box<dyn std::error::Error>> {
    let raw = r#"
[[profiles]]
name = "day"

[[profiles.outputs]]
output = "eDP-1"
video = "/videos/laptop-day.mp4"

[[profiles.outputs]]
output = "HDMI-A-1"
video = "/videos/external-day.mp4"
"#;
    let config: ProfilesConfig = toml::from_str(raw)?;
    let targets = config.profiles[0].render_targets();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].output.as_deref(), Some("eDP-1"));
    assert_eq!(targets[0].video, "/videos/laptop-day.mp4");
    assert_eq!(targets[1].output.as_deref(), Some("HDMI-A-1"));
    assert_eq!(targets[1].video, "/videos/external-day.mp4");
    Ok(())
}

#[test]
fn ensure_config_exists_writes_blank_default_template() -> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let path = temp.path().join("profiles.toml");

    assert!(ensure_config_exists(&path)?);
    assert!(!ensure_config_exists(&path)?);

    let config = ProfilesConfig::load(&path)?;
    assert_eq!(config.settings.default_profile.as_deref(), Some("blank"));
    let blank = config
        .profiles
        .iter()
        .find(|profile| profile.name == "blank")
        .ok_or_else(|| io::Error::other("blank profile not found in generated template"))?;
    assert_eq!(blank.video, "blank://");

    Ok(())
}

#[derive(Clone)]
struct FakeProcess {
    terminated: Rc<RefCell<usize>>,
}

impl PlaybackProcess for FakeProcess {
    fn terminate(&mut self) {
        *self.terminated.borrow_mut() += 1;
    }
}

#[derive(Clone)]
struct FakeLauncher {
    spawns: SpawnLog,
    terminated: Rc<RefCell<usize>>,
}

impl PlaybackLauncher for FakeLauncher {
    type Process = FakeProcess;

    fn spawn_play_process(
        &self,
        input: &str,
        _loop_playback: bool,
        output: Option<&str>,
        _mute: bool,
    ) -> Result<Self::Process, io::Error> {
        self.spawns
            .borrow_mut()
            .push((input.to_string(), output.map(ToOwned::to_owned)));
        Ok(FakeProcess {
            terminated: self.terminated.clone(),
        })
    }
}

#[derive(Clone)]
struct InMemoryStore {
    override_value: Rc<RefCell<Option<String>>>,
}

impl OverrideStore for InMemoryStore {
    fn read_manual_override(&self, _path: &Path) -> Result<Option<String>, io::Error> {
        Ok(self.override_value.borrow().clone())
    }

    fn write_manual_override(&self, _path: &Path, profile: Option<&str>) -> Result<(), io::Error> {
        *self.override_value.borrow_mut() = profile.map(ToOwned::to_owned);
        Ok(())
    }
}

#[derive(Clone)]
struct FixedClock {
    now: chrono::DateTime<Local>,
}

impl TimeProvider for FixedClock {
    fn now(&self) -> chrono::DateTime<Local> {
        self.now
    }
}

#[test]
fn auto_controller_switches_profile_via_public_trait_api() -> Result<(), Box<dyn std::error::Error>>
{
    let config = ProfilesConfig {
        settings: Settings {
            check_interval_seconds: 1,
            default_profile: Some("day".to_string()),
            override_file: None,
            mute: false,
        },
        profiles: vec![
            Profile {
                name: "day".to_string(),
                video: "day.mp4".to_string(),
                outputs: Vec::new(),
                schedule: None,
            },
            Profile {
                name: "night".to_string(),
                video: "night.mp4".to_string(),
                outputs: Vec::new(),
                schedule: None,
            },
        ],
    };

    let spawns = Rc::new(RefCell::new(Vec::<(String, Option<String>)>::new()));
    let terminated = Rc::new(RefCell::new(0usize));
    let override_value = Rc::new(RefCell::new(None));

    let launcher = FakeLauncher {
        spawns: spawns.clone(),
        terminated: terminated.clone(),
    };
    let store = InMemoryStore {
        override_value: override_value.clone(),
    };
    let clock = FixedClock {
        now: Local
            .with_ymd_and_hms(2026, 2, 18, 12, 0, 0)
            .single()
            .expect("valid fixed timestamp"),
    };

    let mut controller = AutoController::new(launcher, store, clock);
    let override_path = Path::new("ignored.override");

    let first = controller.tick(&config, override_path)?;
    assert!(first.changed);
    assert_eq!(first.active_profile_name, "day");

    let second = controller.tick(&config, override_path)?;
    assert!(!second.changed);

    controller.write_manual_override(override_path, Some("night"))?;
    let third = controller.tick(&config, override_path)?;
    assert!(third.changed);
    assert_eq!(third.active_profile_name, "night");

    controller.shutdown();

    assert_eq!(
        spawns.borrow().as_slice(),
        &[
            ("day.mp4".to_string(), None),
            ("night.mp4".to_string(), None),
        ]
    );
    assert_eq!(*terminated.borrow(), 2);

    Ok(())
}

#[test]
fn auto_controller_spawns_per_output_targets() -> Result<(), Box<dyn std::error::Error>> {
    let config = ProfilesConfig {
        settings: Settings {
            check_interval_seconds: 1,
            default_profile: Some("day".to_string()),
            override_file: None,
            mute: false,
        },
        profiles: vec![
            Profile {
                name: "day".to_string(),
                video: "day.mp4".to_string(),
                outputs: vec![
                    ProfileOutput {
                        output: "eDP-1".to_string(),
                        video: "day-laptop.mp4".to_string(),
                    },
                    ProfileOutput {
                        output: "HDMI-A-1".to_string(),
                        video: "day-external.mp4".to_string(),
                    },
                ],
                schedule: None,
            },
            Profile {
                name: "night".to_string(),
                video: "night.mp4".to_string(),
                outputs: vec![ProfileOutput {
                    output: "HDMI-A-1".to_string(),
                    video: "night-external.mp4".to_string(),
                }],
                schedule: None,
            },
        ],
    };

    let spawns = Rc::new(RefCell::new(Vec::<(String, Option<String>)>::new()));
    let terminated = Rc::new(RefCell::new(0usize));
    let override_value = Rc::new(RefCell::new(None));

    let launcher = FakeLauncher {
        spawns: spawns.clone(),
        terminated: terminated.clone(),
    };
    let store = InMemoryStore {
        override_value: override_value.clone(),
    };
    let clock = FixedClock {
        now: Local
            .with_ymd_and_hms(2026, 2, 18, 12, 0, 0)
            .single()
            .expect("valid fixed timestamp"),
    };

    let mut controller = AutoController::new(launcher, store, clock);
    let override_path = Path::new("ignored.override");

    let first = controller.tick(&config, override_path)?;
    assert!(first.changed);
    assert_eq!(first.active_profile_name, "day");
    assert_eq!(
        first.active_video,
        "eDP-1=day-laptop.mp4, HDMI-A-1=day-external.mp4"
    );

    controller.write_manual_override(override_path, Some("night"))?;
    let second = controller.tick(&config, override_path)?;
    assert!(second.changed);
    assert_eq!(second.active_profile_name, "night");
    assert_eq!(second.active_video, "HDMI-A-1=night-external.mp4");

    controller.shutdown();

    assert_eq!(
        spawns.borrow().as_slice(),
        &[
            ("day-laptop.mp4".to_string(), Some("eDP-1".to_string())),
            ("day-external.mp4".to_string(), Some("HDMI-A-1".to_string())),
            (
                "night-external.mp4".to_string(),
                Some("HDMI-A-1".to_string())
            ),
        ]
    );
    assert_eq!(*terminated.borrow(), 3);

    Ok(())
}
