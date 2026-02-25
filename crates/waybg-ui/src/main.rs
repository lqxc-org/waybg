use clap::{ArgAction, Parser, Subcommand};
use std::{env, path::PathBuf};
use waybg_core::default_config_path;
use waybg_ui::GuiRuntimeOptions;

#[derive(Parser, Debug)]
#[command(name = "waybg-ui", version, about = "Waybg Freya UI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the Freya profile controller UI.
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Internal playback entrypoint used by UI preview.
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
            let options = GuiRuntimeOptions::new(config, executable, vec!["play".to_string()]);
            waybg_ui::run_gui(options)?;
            Ok(())
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
