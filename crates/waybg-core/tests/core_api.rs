use chrono::{Local, TimeZone};
use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    rc::Rc,
};
use tempfile::TempDir;
use waybg_core::{
    AutoController, OverrideStore, PlaybackLauncher, PlaybackProcess, Profile, ProfilesConfig,
    ScheduleWindow, Settings, TimeProvider, ensure_config_exists, read_manual_override,
    resolve_override_path, write_manual_override,
};

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
fn resolve_override_path_uses_relative_path_from_config_dir() {
    let config_path = Path::new("/tmp/waybg/profiles.toml");
    let config = ProfilesConfig {
        settings: Settings {
            check_interval_seconds: 15,
            default_profile: None,
            override_file: Some("state/current.override".to_string()),
        },
        profiles: vec![Profile {
            name: "fallback".to_string(),
            video: "fallback.mp4".to_string(),
            schedule: None,
        }],
    };

    let resolved = resolve_override_path(config_path, &config);
    assert_eq!(resolved, PathBuf::from("/tmp/waybg/state/current.override"));
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
    spawns: Rc<RefCell<Vec<String>>>,
    terminated: Rc<RefCell<usize>>,
}

impl PlaybackLauncher for FakeLauncher {
    type Process = FakeProcess;

    fn spawn_play_process(
        &self,
        input: &str,
        _loop_playback: bool,
    ) -> Result<Self::Process, io::Error> {
        self.spawns.borrow_mut().push(input.to_string());
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
        },
        profiles: vec![
            Profile {
                name: "day".to_string(),
                video: "day.mp4".to_string(),
                schedule: None,
            },
            Profile {
                name: "night".to_string(),
                video: "night.mp4".to_string(),
                schedule: None,
            },
        ],
    };

    let spawns = Rc::new(RefCell::new(Vec::new()));
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

    assert_eq!(spawns.borrow().as_slice(), &["day.mp4", "night.mp4"]);
    assert_eq!(*terminated.borrow(), 2);

    Ok(())
}
