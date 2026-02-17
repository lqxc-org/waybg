use std::{
    env,
    error::Error,
    io,
    path::PathBuf,
    process::{Command, ExitStatus},
};

type DynError = Box<dyn Error>;

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), DynError> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("normal-tests") => normal_tests(),
        Some("niri-tests") => niri_tests(),
        Some("help") | None => {
            print_help();
            Ok(())
        }
        Some(other) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown xtask command: {other}"),
        )
        .into()),
    }
}

fn print_help() {
    println!(
        "Usage:\n  cargo xtask normal-tests\n  cargo xtask niri-tests\n\nAliases:\n  cargo normal-tests\n  cargo niri-tests"
    );
}

fn normal_tests() -> Result<(), DynError> {
    println!("[xtask] Running normal test suite (embedded Niri E2E disabled)");
    let status = cargo_cmd(
        &[
            "test",
            "--workspace",
            "--exclude",
            "xtask",
            "--",
            "--skip",
            "niri_embedded_session_renders_video_pixels",
        ],
        &[("WAYSTREAM_E2E_NIRI", "0")],
    )?;
    ensure_success(
        "cargo test --workspace --exclude xtask -- --skip niri_embedded_session_renders_video_pixels",
        status,
    )
}

fn niri_tests() -> Result<(), DynError> {
    println!("[xtask] Running embedded Niri integration test");
    let status = cargo_cmd(
        &["test", "--test", "niri_embedded", "--", "--nocapture"],
        &[("WAYSTREAM_E2E_NIRI", "1")],
    )?;
    ensure_success(
        "WAYSTREAM_E2E_NIRI=1 cargo test --test niri_embedded -- --nocapture",
        status,
    )
}

fn cargo_cmd(args: &[&str], extra_env: &[(&str, &str)]) -> Result<ExitStatus, io::Error> {
    let mut command = Command::new("cargo");
    command.args(args).current_dir(workspace_root());

    for (key, value) in extra_env {
        command.env(key, value);
    }

    command.status()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under workspace root")
        .to_path_buf()
}

fn ensure_success(operation: &str, status: ExitStatus) -> Result<(), DynError> {
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("command failed: {operation} (status {status})")).into())
    }
}
