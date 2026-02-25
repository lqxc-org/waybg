use clap::{ArgAction, Parser, Subcommand};
use std::{env, path::PathBuf};
use waybg_core::{DynError, default_config_path, write_example_config};
use waybg_ui::GuiRuntimeOptions;

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
        #[arg(long)]
        output: Option<String>,
        #[arg(long)]
        metrics_file: Option<PathBuf>,
        #[arg(long, action = ArgAction::SetTrue)]
        mute: bool,
        #[arg(long, action = ArgAction::SetTrue, conflicts_with = "mute")]
        unmute: bool,
    },
    /// Run automatic profile switching loop based on schedule and manual override.
    Auto {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Open Freya UI for previewing and selecting profiles.
    Gui {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Write a starter profiles config file.
    InitConfig {
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<(), DynError> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Commands::Gui { config: None });

    match command {
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
        Commands::Auto { config } => {
            let config = config.unwrap_or(default_config_path()?);
            let executable = env::current_exe()?;
            waybg_daemon::run_auto_controller(&config, executable, vec!["play".to_string()])
        }
        Commands::Gui { config } => {
            let config = config.unwrap_or(default_config_path()?);
            let executable = env::current_exe()?;
            let options = GuiRuntimeOptions::new(config, executable, vec!["play".to_string()]);
            waybg_ui::run_gui(options)?;
            Ok(())
        }
        Commands::InitConfig { output } => {
            let output = output.unwrap_or(default_config_path()?);
            write_example_config(&output)?;
            println!("Wrote example config to '{}'.", output.display());
            Ok(())
        }
    }
}
