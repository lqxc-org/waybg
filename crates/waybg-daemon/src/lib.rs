use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};
use waybg_core::{
    AutoController, DynError, FsOverrideStore, PlaybackLauncher, PlaybackProcess, ProfilesConfig,
    SystemTimeProvider, ensure_config_exists, resolve_override_path,
};

#[derive(Debug, Clone)]
struct PlayerCommand {
    executable: PathBuf,
    prefix_args: Vec<String>,
}

impl PlayerCommand {
    fn spawn_play_process(&self, input: &str, loop_playback: bool) -> Result<Child, io::Error> {
        let mut command = Command::new(&self.executable);
        command.args(&self.prefix_args).arg(input);
        if loop_playback {
            command.arg("--loop-playback");
        }

        command
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
    }
}

#[derive(Debug, Clone)]
struct CommandPlaybackLauncher {
    player: PlayerCommand,
}

struct ChildPlayProcess {
    child: Child,
}

impl PlaybackProcess for ChildPlayProcess {
    fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl PlaybackLauncher for CommandPlaybackLauncher {
    type Process = ChildPlayProcess;

    fn spawn_play_process(
        &self,
        input: &str,
        loop_playback: bool,
    ) -> Result<Self::Process, io::Error> {
        let child = self.player.spawn_play_process(input, loop_playback)?;
        Ok(ChildPlayProcess { child })
    }
}

pub fn run_auto_controller(
    config_path: &Path,
    player_executable: PathBuf,
    player_prefix_args: Vec<String>,
) -> Result<(), DynError> {
    if ensure_config_exists(config_path)? {
        println!(
            "Config '{}' did not exist; generated an example config with blank fallback profile.",
            config_path.display()
        );
    }

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

    let launcher = CommandPlaybackLauncher {
        player: PlayerCommand {
            executable: player_executable,
            prefix_args: player_prefix_args,
        },
    };
    let store = FsOverrideStore;
    let clock = SystemTimeProvider;
    let mut controller = AutoController::new(launcher, store, clock);

    loop {
        let tick = controller.tick(&config, &override_path)?;
        if tick.changed {
            println!(
                "{} active profile -> '{}' ({})",
                tick.timestamp.format("%Y-%m-%d %H:%M:%S"),
                tick.active_profile_name,
                tick.active_video
            );
        }
        thread::sleep(interval);
    }
}
