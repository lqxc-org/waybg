use clap::{Parser, Subcommand};
use std::{env, path::PathBuf};
use waybg_core::{DynError, write_example_config};
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
        #[arg(long, default_value = DEFAULT_CONFIG)]
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
        Commands::InitConfig { output } => {
            write_example_config(&output)?;
            println!("Wrote example config to '{}'.", output.display());
            Ok(())
        }
    }
}
