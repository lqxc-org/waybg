use clap::{Parser, Subcommand};
use std::{
    env,
    path::{Path, PathBuf},
};
use waybg_core::DynError;
use waybg_ui::GuiRuntimeOptions;

const DEFAULT_CONFIG: &str = "profiles.toml";

#[derive(Parser, Debug)]
#[command(name = "waybg", version, about = "Wayland video wallpaper controller")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Play one video directly using the Wayland core renderer.
    Play {
        input: String,
        #[arg(long, default_value_t = false)]
        loop_playback: bool,
    },
    /// Run automatic profile switching loop based on schedule and manual override.
    Auto {
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Open Freya UI for previewing and selecting profiles.
    Gui {
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Write a starter profiles config file.
    InitConfig {
        #[arg(long, default_value = "profiles.example.toml")]
        output: PathBuf,
    },
}

fn main() -> Result<(), DynError> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Gui {
        config: PathBuf::from(DEFAULT_CONFIG),
    });

    match command {
        Commands::Play {
            input,
            loop_playback,
        } => wayland_core::play_video(&input, loop_playback),
        Commands::Auto { config } => {
            let executable = env::current_exe()?;
            waybg_daemon::run_auto_controller(&config, executable, vec!["play".to_string()])
        }
        Commands::Gui { config } => {
            let executable = env::current_exe()?;
            let options = GuiRuntimeOptions::new(config, executable, vec!["play".to_string()]);
            waybg_ui::run_gui(options);
            Ok(())
        }
        Commands::InitConfig { output } => write_example_config(&output),
    }
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
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, TEMPLATE)?;
    println!("Wrote example config to '{}'.", output.display());
    Ok(())
}
