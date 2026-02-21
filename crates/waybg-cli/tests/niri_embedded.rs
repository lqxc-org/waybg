use image::ImageReader;
use std::{
    env,
    error::Error,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

#[test]
fn niri_embedded_session_renders_video_pixels() -> Result<(), Box<dyn Error>> {
    if !should_run_e2e() {
        eprintln!(
            "Skipping Niri embedded test. Set WAYSTREAM_E2E_NIRI=1 to run compositor E2E checks."
        );
        return Ok(());
    }

    if env::var_os("WAYLAND_DISPLAY").is_none() && env::var_os("DISPLAY").is_none() {
        eprintln!("Skipping Niri embedded test. Neither WAYLAND_DISPLAY nor DISPLAY is available.");
        return Ok(());
    }

    ensure_tools_exist(&["niri", "grim", "ffmpeg", "sh"])?;

    let temp = TempDir::new()?;
    let temp_path = temp.path();
    let video_path = temp_path.join("solid_red.mp4");
    let screenshot_path = temp_path.join("frame.png");
    let niri_log_path = temp_path.join("niri.log");
    let app_log_path = temp_path.join("waybg.log");
    let niri_config_path = temp_path.join("niri-test.kdl");

    fs::write(
        &niri_config_path,
        r#"xwayland-satellite {
    off
}
"#,
    )?;

    generate_solid_color_video(&video_path, "red", 640, 360, 4)?;

    let waybg_bin = waybg_binary_path()?;
    let script = r#"
set -euo pipefail
"$WAYBG_BIN" play "$VIDEO_PATH" --loop-playback >"$APP_LOG" 2>&1 &
APP_PID=$!
cleanup() {
  kill "$APP_PID" 2>/dev/null || true
  wait "$APP_PID" 2>/dev/null || true
}
trap cleanup EXIT

# Give the client enough time to create and render its first frame.
sleep 3

grim "$SCREENSHOT_PATH"
"#;

    let niri_log = File::create(&niri_log_path)?;
    let niri_log_err = niri_log.try_clone()?;

    let mut niri = Command::new("niri")
        .arg("-c")
        .arg(&niri_config_path)
        .arg("--")
        .arg("sh")
        .arg("-lc")
        .arg(script)
        .env("WAYBG_BIN", waybg_bin)
        .env("VIDEO_PATH", &video_path)
        .env("SCREENSHOT_PATH", &screenshot_path)
        .env("APP_LOG", &app_log_path)
        .stdout(Stdio::from(niri_log))
        .stderr(Stdio::from(niri_log_err))
        .spawn()?;

    if let Err(error) =
        wait_for_screenshot_or_exit(&mut niri, &screenshot_path, Duration::from_secs(30))
    {
        let niri_log = fs::read_to_string(&niri_log_path).unwrap_or_default();
        let app_log = fs::read_to_string(&app_log_path).unwrap_or_default();
        return Err(format!(
            "nested niri session did not produce a screenshot: {error}\n--- niri.log ---\n{niri_log}\n--- waybg.log ---\n{app_log}"
        )
        .into());
    }

    let _ = niri.kill();
    let _ = niri.wait();

    if !screenshot_path.exists() {
        let niri_log = fs::read_to_string(&niri_log_path).unwrap_or_default();
        let app_log = fs::read_to_string(&app_log_path).unwrap_or_default();
        return Err(format!(
            "no screenshot captured at '{}'\n--- niri.log ---\n{niri_log}\n--- waybg.log ---\n{app_log}",
            screenshot_path.display()
        )
        .into());
    }

    let metrics = screenshot_metrics(&screenshot_path)?;
    assert!(
        metrics.red_ratio > 0.01,
        "expected at least 1% red pixels from test video, got {:.4}% ({}x{})",
        metrics.red_ratio * 100.0,
        metrics.width,
        metrics.height
    );
    assert!(
        metrics.non_black_ratio > 0.05,
        "expected rendered non-black pixels in screenshot, got {:.4}% ({}x{})",
        metrics.non_black_ratio * 100.0,
        metrics.width,
        metrics.height
    );

    Ok(())
}

fn should_run_e2e() -> bool {
    match env::var("WAYSTREAM_E2E_NIRI") {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"),
        Err(_) => false,
    }
}

fn ensure_tools_exist(tools: &[&str]) -> Result<(), io::Error> {
    for tool in tools {
        if !command_exists(tool) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("required tool not found in PATH: {tool}"),
            ));
        }
    }
    Ok(())
}

fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn generate_solid_color_video(
    output: &Path,
    color: &str,
    width: u32,
    height: u32,
    duration_seconds: u32,
) -> Result<(), Box<dyn Error>> {
    let lavfi = format!("color=c={color}:s={width}x{height}:d={duration_seconds}");
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg(lavfi)
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if !status.success() {
        return Err(format!(
            "ffmpeg failed to generate test video at '{}'",
            output.display()
        )
        .into());
    }
    Ok(())
}

fn waybg_binary_path() -> Result<PathBuf, Box<dyn Error>> {
    env::var("CARGO_BIN_EXE_waybg")
        .map(PathBuf::from)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "CARGO_BIN_EXE_waybg is missing; run as integration test via cargo test",
            )
        })
        .map_err(Into::into)
}

fn wait_for_screenshot_or_exit(
    child: &mut Child,
    screenshot: &Path,
    timeout: Duration,
) -> Result<(), io::Error> {
    let deadline = Instant::now() + timeout;
    loop {
        if screenshot.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "nested niri exited before screenshot became available (status {status})"
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "timed out after {}s waiting for screenshot '{}'",
                    timeout.as_secs(),
                    screenshot.display()
                ),
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[derive(Debug)]
struct ScreenshotMetrics {
    width: u32,
    height: u32,
    red_ratio: f64,
    non_black_ratio: f64,
}

fn screenshot_metrics(path: &Path) -> Result<ScreenshotMetrics, Box<dyn Error>> {
    let image = ImageReader::open(path)?.decode()?.to_rgb8();
    let (width, height) = image.dimensions();
    let total = (width as u64) * (height as u64);
    if total == 0 {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "screenshot has zero pixels").into(),
        );
    }

    let mut red_pixels = 0u64;
    let mut non_black_pixels = 0u64;

    for pixel in image.pixels() {
        let [r, g, b] = pixel.0;
        if r > 160 && g < 90 && b < 90 {
            red_pixels += 1;
        }
        if u16::from(r) + u16::from(g) + u16::from(b) > 30 {
            non_black_pixels += 1;
        }
    }

    Ok(ScreenshotMetrics {
        width,
        height,
        red_ratio: red_pixels as f64 / total as f64,
        non_black_ratio: non_black_pixels as f64 / total as f64,
    })
}
