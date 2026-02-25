use clap::{ArgAction, Parser, Subcommand};
use std::{env, path::PathBuf};
use waybg_core::default_config_path;

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
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Internal playback entrypoint used by the daemon itself.
    Play {
        input: String,
        #[arg(long, default_value_t = false)]
        loop_playback: bool,
        #[arg(long)]
        output: Option<String>,
        #[arg(long)]
        metrics_file: Option<PathBuf>,
        #[arg(long, action = ArgAction::SetTrue)]
        mute: bool,
        #[arg(long, action = ArgAction::SetTrue, conflicts_with = "mute")]
        unmute: bool,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Run { config: None });

    match command {
        Commands::Run { config } => {
            let config = config.unwrap_or(default_config_path()?);
            let executable = env::current_exe()?;
            waybg_daemon::run_auto_controller(&config, executable, vec!["play".to_string()])
        }
        Commands::Play {
            input,
            loop_playback,
            output,
            metrics_file,
            mute,
            unmute,
        } => {
            let mute = if unmute { false } else { mute };
            wayland_core::play_video(
                &input,
                loop_playback,
                output.as_deref(),
                mute,
                metrics_file.as_deref(),
            )
        }
    }
}
