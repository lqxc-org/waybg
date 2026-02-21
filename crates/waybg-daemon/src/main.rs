use clap::{Parser, Subcommand};
use std::{env, path::PathBuf};

const DEFAULT_CONFIG: &str = "profiles.toml";

#[derive(Parser, Debug)]
#[command(name = "waybg-daemon", version, about = "Waybg daemon process")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run automatic profile switching loop based on schedule and manual override.
    Run {
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Internal playback entrypoint used by the daemon itself.
    Play {
        input: String,
        #[arg(long, default_value_t = false)]
        loop_playback: bool,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Run {
        config: PathBuf::from(DEFAULT_CONFIG),
    });

    match command {
        Commands::Run { config } => {
            let executable = env::current_exe()?;
            waybg_daemon::run_auto_controller(&config, executable, vec!["play".to_string()])
        }
        Commands::Play {
            input,
            loop_playback,
        } => wayland_core::play_video(&input, loop_playback),
    }
}
