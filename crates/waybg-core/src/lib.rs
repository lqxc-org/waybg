use chrono::{DateTime, Datelike, Local, NaiveTime};
use serde::{Deserialize, Serialize};
use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

pub type DynError = Box<dyn Error>;
pub const APP_DIR_NAME: &str = "waybg";
pub const DEFAULT_CONFIG_FILENAME: &str = "profiles.toml";
pub const DEFAULT_OVERRIDE_FILENAME: &str = "profiles.override";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ProfilesConfig {
    #[serde(default)]
    pub settings: Settings,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct Settings {
    #[serde(default = "default_check_interval_seconds")]
    pub check_interval_seconds: u64,
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub override_file: Option<String>,
    #[serde(default)]
    pub mute: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub name: String,
    #[serde(default = "default_profile_video")]
    pub video: String,
    #[serde(default)]
    pub outputs: Vec<ProfileOutput>,
    #[serde(default)]
    pub schedule: Option<ScheduleWindow>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProfileOutput {
    pub output: String,
    #[serde(default = "default_profile_video")]
    pub video: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderTarget {
    pub output: Option<String>,
    pub video: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ScheduleWindow {
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub weekdays: Vec<u32>,
}

pub fn default_check_interval_seconds() -> u64 {
    15
}

pub fn default_profile_video() -> String {
    "blank://".to_string()
}

pub fn default_config_path() -> Result<PathBuf, io::Error> {
    Ok(resolve_xdg_base_dir("XDG_CONFIG_HOME", ".config")?
        .join(APP_DIR_NAME)
        .join(DEFAULT_CONFIG_FILENAME))
}

pub fn default_override_path() -> Result<PathBuf, io::Error> {
    Ok(resolve_xdg_base_dir("XDG_STATE_HOME", ".local/state")?
        .join(APP_DIR_NAME)
        .join(DEFAULT_OVERRIDE_FILENAME))
}

pub const EXAMPLE_CONFIG_TEMPLATE: &str = r#"[settings]
check_interval_seconds = 15
default_profile = "blank"
# override_file = "/absolute/path/to/custom.override"
# mute = false

[[profiles]]
name = "day"
video = "/absolute/path/to/day.mp4"
# Optional per-output videos for multi-monitor:
# [[profiles.outputs]]
# output = "eDP-1"
# video = "/absolute/path/to/laptop-day.mp4"
# [[profiles.outputs]]
# output = "HDMI-A-1"
# video = "/absolute/path/to/external-day.mp4"
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
name = "blank"
# `video` is optional. If omitted, waybg uses blank:// (solid black background).
"#;

pub fn write_example_config(output: &Path) -> Result<(), io::Error> {
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, EXAMPLE_CONFIG_TEMPLATE)
}

pub fn ensure_config_exists(path: &Path) -> Result<bool, io::Error> {
    if path.exists() {
        return Ok(false);
    }
    write_example_config(path)?;
    Ok(true)
}

impl ProfilesConfig {
    pub fn load(path: &Path) -> Result<Self, DynError> {
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

    pub fn pick_profile<'a>(
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

impl Profile {
    pub fn render_targets(&self) -> Vec<RenderTarget> {
        if self.outputs.is_empty() {
            vec![RenderTarget {
                output: None,
                video: self.video.clone(),
            }]
        } else {
            self.outputs
                .iter()
                .map(|output| RenderTarget {
                    output: Some(output.output.clone()),
                    video: output.video.clone(),
                })
                .collect()
        }
    }
}

pub fn summarize_render_targets(targets: &[RenderTarget]) -> String {
    if targets.is_empty() {
        return "<no targets>".to_string();
    }

    if targets.len() == 1 && targets[0].output.is_none() {
        return targets[0].video.clone();
    }

    targets
        .iter()
        .map(|target| match &target.output {
            Some(output) => format!("{output}={}", target.video),
            None => format!("default={}", target.video),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl ScheduleWindow {
    pub fn is_active(&self, now: DateTime<Local>) -> bool {
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

pub fn resolve_override_path(
    config_path: &Path,
    config: &ProfilesConfig,
) -> Result<PathBuf, io::Error> {
    match config.settings.override_file.as_deref() {
        Some(path) => {
            let custom = PathBuf::from(path);
            if custom.is_absolute() {
                Ok(custom)
            } else {
                Ok(config_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(custom))
            }
        }
        None => default_override_path(),
    }
}

fn resolve_xdg_base_dir(env_var: &str, default_home_suffix: &str) -> Result<PathBuf, io::Error> {
    if let Some(value) = env::var_os(env_var)
        && !value.is_empty()
    {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("environment variable '{env_var}' must be an absolute path"),
            ));
        }
        return Ok(path);
    }

    let home = env::var_os("HOME").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("cannot resolve '{env_var}': HOME is not set"),
        )
    })?;
    let home_path = PathBuf::from(home);
    if !home_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "environment variable 'HOME' must be an absolute path",
        ));
    }

    Ok(home_path.join(default_home_suffix))
}

pub fn read_manual_override(path: &Path) -> Result<Option<String>, io::Error> {
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

pub fn write_manual_override(path: &Path, profile: Option<&str>) -> Result<(), io::Error> {
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

fn parse_hhmm(input: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(input, "%H:%M").ok()
}

pub trait PlaybackProcess {
    fn terminate(&mut self);
}

pub trait PlaybackLauncher {
    type Process: PlaybackProcess;

    fn spawn_play_process(
        &self,
        input: &str,
        loop_playback: bool,
        output: Option<&str>,
        mute: bool,
    ) -> Result<Self::Process, io::Error>;
}

pub trait OverrideStore {
    fn read_manual_override(&self, path: &Path) -> Result<Option<String>, io::Error>;
    fn write_manual_override(&self, path: &Path, profile: Option<&str>) -> Result<(), io::Error>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FsOverrideStore;

impl OverrideStore for FsOverrideStore {
    fn read_manual_override(&self, path: &Path) -> Result<Option<String>, io::Error> {
        read_manual_override(path)
    }

    fn write_manual_override(&self, path: &Path, profile: Option<&str>) -> Result<(), io::Error> {
        write_manual_override(path, profile)
    }
}

pub trait TimeProvider {
    fn now(&self) -> DateTime<Local>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTimeProvider;

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> DateTime<Local> {
        Local::now()
    }
}

#[derive(Debug, Clone)]
pub struct AutoTick {
    pub timestamp: DateTime<Local>,
    pub active_profile_name: String,
    pub active_video: String,
    pub changed: bool,
}

pub struct AutoController<L, S, C>
where
    L: PlaybackLauncher,
    S: OverrideStore,
    C: TimeProvider,
{
    launcher: L,
    override_store: S,
    clock: C,
    active_profile_name: Option<String>,
    active_render_signature: Option<String>,
    active_mute: Option<bool>,
    running_processes: Vec<L::Process>,
}

impl<L, S, C> AutoController<L, S, C>
where
    L: PlaybackLauncher,
    S: OverrideStore,
    C: TimeProvider,
{
    pub fn new(launcher: L, override_store: S, clock: C) -> Self {
        Self {
            launcher,
            override_store,
            clock,
            active_profile_name: None,
            active_render_signature: None,
            active_mute: None,
            running_processes: Vec::new(),
        }
    }

    pub fn active_profile_name(&self) -> Option<&str> {
        self.active_profile_name.as_deref()
    }

    pub fn write_manual_override(
        &self,
        path: &Path,
        profile: Option<&str>,
    ) -> Result<(), io::Error> {
        self.override_store.write_manual_override(path, profile)
    }

    pub fn tick(
        &mut self,
        config: &ProfilesConfig,
        override_path: &Path,
    ) -> Result<AutoTick, DynError> {
        let manual_override = self.override_store.read_manual_override(override_path)?;
        let now = self.clock.now();
        let profile = config
            .pick_profile(manual_override.as_deref(), now)
            .ok_or_else(|| io::Error::other("unable to resolve an active profile"))?;
        let targets = profile.render_targets();
        let active_video = summarize_render_targets(&targets);
        let mute = config.settings.mute;

        let mut changed = false;
        let should_restart = self.active_profile_name.as_deref() != Some(profile.name.as_str())
            || self.active_render_signature.as_deref() != Some(active_video.as_str())
            || self.active_mute != Some(mute);
        if should_restart {
            for mut process in self.running_processes.drain(..) {
                process.terminate();
            }

            let mut started_processes = Vec::with_capacity(targets.len());
            for target in &targets {
                match self.launcher.spawn_play_process(
                    &target.video,
                    true,
                    target.output.as_deref(),
                    mute,
                ) {
                    Ok(process) => started_processes.push(process),
                    Err(error) => {
                        for mut process in started_processes {
                            process.terminate();
                        }
                        return Err(error.into());
                    }
                }
            }

            self.running_processes = started_processes;
            self.active_profile_name = Some(profile.name.clone());
            self.active_render_signature = Some(active_video.clone());
            self.active_mute = Some(mute);
            changed = true;
        }

        Ok(AutoTick {
            timestamp: now,
            active_profile_name: profile.name.clone(),
            active_video,
            changed,
        })
    }

    pub fn shutdown(&mut self) {
        for mut process in self.running_processes.drain(..) {
            process.terminate();
        }
        self.active_profile_name = None;
        self.active_render_signature = None;
        self.active_mute = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::{cell::RefCell, rc::Rc};

    type SpawnLog = Rc<RefCell<Vec<(String, Option<String>)>>>;

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
    struct FakeStore {
        override_value: Rc<RefCell<Option<String>>>,
    }

    impl OverrideStore for FakeStore {
        fn read_manual_override(&self, _path: &Path) -> Result<Option<String>, io::Error> {
            Ok(self.override_value.borrow().clone())
        }

        fn write_manual_override(
            &self,
            _path: &Path,
            profile: Option<&str>,
        ) -> Result<(), io::Error> {
            *self.override_value.borrow_mut() = profile.map(ToOwned::to_owned);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeClock {
        now: DateTime<Local>,
    }

    impl TimeProvider for FakeClock {
        fn now(&self) -> DateTime<Local> {
            self.now
        }
    }

    #[test]
    fn auto_controller_switches_profiles_using_override() {
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
        let store = FakeStore {
            override_value: override_value.clone(),
        };
        let clock = FakeClock {
            now: Local
                .with_ymd_and_hms(2026, 2, 18, 12, 0, 0)
                .single()
                .expect("valid fixed timestamp"),
        };

        let mut controller = AutoController::new(launcher, store, clock);
        let override_path = Path::new("unused.override");

        let first = controller
            .tick(&config, override_path)
            .expect("first tick should work");
        assert!(first.changed);
        assert_eq!(first.active_profile_name, "day");

        let second = controller
            .tick(&config, override_path)
            .expect("second tick should work");
        assert!(!second.changed);
        assert_eq!(second.active_profile_name, "day");

        *override_value.borrow_mut() = Some("night".to_string());
        let third = controller
            .tick(&config, override_path)
            .expect("third tick should work");
        assert!(third.changed);
        assert_eq!(third.active_profile_name, "night");

        assert_eq!(
            spawns.borrow().as_slice(),
            &[
                ("day.mp4".to_string(), None),
                ("night.mp4".to_string(), None),
            ]
        );
        assert_eq!(*terminated.borrow(), 1);
    }

    #[test]
    fn auto_controller_restarts_when_mute_changes() {
        let mut config = ProfilesConfig {
            settings: Settings {
                check_interval_seconds: 1,
                default_profile: Some("day".to_string()),
                override_file: None,
                mute: false,
            },
            profiles: vec![Profile {
                name: "day".to_string(),
                video: "day.mp4".to_string(),
                outputs: Vec::new(),
                schedule: None,
            }],
        };

        let spawns = Rc::new(RefCell::new(Vec::<(String, Option<String>)>::new()));
        let terminated = Rc::new(RefCell::new(0usize));
        let override_value = Rc::new(RefCell::new(None));
        let launcher = FakeLauncher {
            spawns: spawns.clone(),
            terminated: terminated.clone(),
        };
        let store = FakeStore { override_value };
        let clock = FakeClock {
            now: Local
                .with_ymd_and_hms(2026, 2, 18, 12, 0, 0)
                .single()
                .expect("valid fixed timestamp"),
        };
        let mut controller = AutoController::new(launcher, store, clock);
        let override_path = Path::new("unused.override");

        let first = controller
            .tick(&config, override_path)
            .expect("first tick should work");
        assert!(first.changed);

        let second = controller
            .tick(&config, override_path)
            .expect("second tick should work");
        assert!(!second.changed);

        config.settings.mute = true;
        let third = controller
            .tick(&config, override_path)
            .expect("third tick should work");
        assert!(third.changed);

        assert_eq!(spawns.borrow().len(), 2);
        assert_eq!(*terminated.borrow(), 1);
    }
}
