use clap::{Parser, Subcommand};
use std::{env, path::PathBuf};
use waybg_ui::GuiRuntimeOptions;

const DEFAULT_CONFIG: &str = "profiles.toml";

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
        #[arg(long, default_value = DEFAULT_CONFIG)]
        config: PathBuf,
    },
    /// Internal playback entrypoint used by UI preview.
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
            let options = GuiRuntimeOptions::new(config, executable, vec!["play".to_string()]);
            waybg_ui::run_gui(options);
            Ok(())
        }
        Commands::Play {
            input,
            loop_playback,
        } => wayland_core::play_video(&input, loop_playback),
    }
}
