# waybg

Wayland/Niri video wallpaper monorepo.

## Workspace Layout

- `crates/waybg-cli`: user-facing `waybg` command (`play`, `auto`, `gui`, `init-config`).
- `crates/waybg-ui`: Freya profile controller UI crate/binary.
- `crates/waybg-daemon`: schedule/override daemon crate/binary.
- `crates/wayland-core`: Wayland playback core (layer-shell background rendering, GStreamer decode, metrics export).
- `crates/waybg-core`: shared profile/config/override scheduling logic.
- `xtask`: CI/dev task runner (`cargo ci`, `cargo normal-tests`, `cargo niri-tests`).

## Features

- `play`: direct playback on Wayland.
- `auto`: automatic profile-based wallpaper switching (time schedules + manual override).
- `multi-monitor`: optional per-output videos (`[[profiles.outputs]]`) with one playback process per output.
- `audio`: mute/unmute control from CLI (`--mute`/`--unmute`) and UI toggle (stored in config).
- `gui`: Freya desktop UI to preview profiles and set/clear manual override.
- `performance`: optional FPS metrics JSON output (`--metrics-file`) and UI FPS chart (Avg/low95/low99).
- `init-config`: generate a starter config at the XDG default path.
- `niri backend`: on Niri sessions, playback defaults to a built-in layer-shell renderer (background layer, not a normal app window).
- `hardware decode preference`: known hardware decoders are promoted in GStreamer rank when available.

## Default Paths (XDG)

- Config: `$XDG_CONFIG_HOME/waybg/profiles.toml`
- Manual override state: `$XDG_STATE_HOME/waybg/profiles.override`

All `waybg` binaries use those defaults when `--config` is omitted.
If `XDG_CONFIG_HOME` is unset/empty, waybg uses `$HOME/.config`.
If `XDG_STATE_HOME` is unset/empty, waybg uses `$HOME/.local/state`.
When set, `XDG_CONFIG_HOME` and `XDG_STATE_HOME` must be absolute paths.

On UI startup, if config is missing it is auto-created from the example template, then the active profile from config (schedule + manual override + mute) is applied immediately. If startup validation/apply fails, the app exits with a non-zero status and shows a desktop notification.

## System Dependencies

Install dependencies on Arch Linux:

```bash
sudo pacman -S --needed \
  pkgconf \
  fontconfig \
  freetype2 \
  wayland \
  libxkbcommon \
  gstreamer \
  gst-plugins-base \
  gst-plugins-good \
  gst-plugins-bad \
  gst-plugins-ugly \
  gst-libav \
  ffmpeg
```

`gst-libav` lets `playbin` use local FFmpeg decoders, including common codecs and AV1 (`avdec_av1` when available).
On Niri, waybg defaults to its internal layer-shell backend so wallpaper playback does not appear as a normal window.
Use `WAYBG_BACKEND=gstreamer` to force the legacy GStreamer window backend, or `WAYBG_BACKEND=layer-shell` to force layer-shell.
Use `WAYBG_SCALE_MODE=fill|fit|stretch` to control layer-shell scaling quality/behavior (`fill` is default, aspect-preserving crop).
Use `WAYBG_DMABUF=auto|on|off` to control dmabuf import (`auto` default; `on` fails fast if compositor/dma_heap support is missing).
When decoder/appsink negotiate `video/x-raw(memory:DMABuf),format=DMA_DRM` (preferred) or `video/x-raw(memory:DMABuf),format=BGRA`, waybg imports decoder dmabuf frames directly into layer-shell buffers (including multi-plane layouts) with automatic fallback when unsupported.
waybg also promotes available hardware decoder plugins (`va*`, `v4l2*`, `nv*`, etc.) to prefer GPU/accelerated decode paths.

## Quick Start

1) Generate sample config:

```bash
cargo run -p waybg-cli -- init-config
```

If you skip this step, `waybg auto` and `waybg gui` will auto-generate the XDG config file when missing.

2) Edit `$XDG_CONFIG_HOME/waybg/profiles.toml` with real video file paths.

3) Start automatic switching:

```bash
cargo run -p waybg-cli -- auto
```

4) Open Freya profile controller (optional, in another terminal):

```bash
cargo run -p waybg-cli -- gui
```

5) Direct one-off playback:

```bash
cargo run -p waybg-cli -- play /path/to/video.mp4 --loop-playback
```

Target a specific monitor/output:

```bash
cargo run -p waybg-cli -- play /path/to/video.mp4 --loop-playback --output HDMI-A-1
```

Play muted (or explicitly unmute):

```bash
cargo run -p waybg-cli -- play /path/to/video.mp4 --loop-playback --mute
cargo run -p waybg-cli -- play /path/to/video.mp4 --loop-playback --unmute
```

Export FPS metrics JSON for UI/monitoring:

```bash
cargo run -p waybg-cli -- play /path/to/video.mp4 --loop-playback --metrics-file /tmp/waybg-fps.json
```

`--metrics-file` writes avg/low95/low99 and recent FPS samples as JSON.

In UI, metrics have both manual and live modes:
- `Enable/Disable Capture`: manual control for whether playback writes metrics files.
- `Trigger Snapshot`: manual pull/reload of the latest metrics.
- `Pause Live` / `Resume Live`: real-time polling mode (default 250ms refresh cadence).

## Optional Standalone Binaries

Run daemon directly:

```bash
cargo run -p waybg-daemon -- run
```

Run UI directly:

```bash
cargo run -p waybg-ui -- run
```

## Config Format

```toml
[settings]
check_interval_seconds = 15
default_profile = "blank"
# override_file = "/absolute/path/to/custom.override"
# mute = false

[[profiles]]
name = "day"
video = "/absolute/path/to/day.mp4"
[profiles.schedule]
start = "08:00"
end = "18:00"
weekdays = [1, 2, 3, 4, 5]

# Optional per-output mapping for multi-monitor. When outputs are present,
# waybg targets only these outputs for this profile.
[[profiles]]
name = "day-multi"
[[profiles.outputs]]
output = "eDP-1"
video = "/absolute/path/to/laptop-day.mp4"
[[profiles.outputs]]
output = "HDMI-A-1"
video = "/absolute/path/to/external-day.mp4"

[[profiles]]
name = "night"
video = "/absolute/path/to/night.mp4"
[profiles.schedule]
start = "18:00"
end = "08:00"

[[profiles]]
name = "blank"
# video is optional; missing video defaults to blank:// (solid black)
```

`weekdays` uses ISO numbering: `1=Mon ... 7=Sun`.

`video` accepts `blank://`, `blank`, or `none` for a solid black background.

Per-output playback uses the `output` name from your compositor (for Niri, names like `eDP-1`, `HDMI-A-1`, etc.). The CLI `play` command also accepts `--output <name>` for one-off targeting.
Use `settings.mute = true` to mute playback in auto mode. The UI mute/unmute button updates this value in the config file.
UI playback writes per-target metrics files under the override directory parent (`.../waybg/metrics/*.json`); `Trigger Snapshot` redraws manually and live mode refreshes automatically.

When `settings.override_file` is not set, waybg writes manual override state to
`$XDG_STATE_HOME/waybg/profiles.override`.

## Download Binary from GitHub Actions Artifact

Every CI run uploads an artifact named:

```text
waybg-archlinux-x86_64-<commit-sha>
```

To download it:

1) Open GitHub `Actions` tab.
2) Open a workflow run for `CI and Release`.
3) In `Artifacts`, download `waybg-archlinux-x86_64-<commit-sha>`.
4) Extract and run the included `waybg-<version>-archlinux-x86_64-elf` binary.

## Pre-commit Checks

This repo includes a Git pre-commit hook in `.githooks/pre-commit` that runs:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Enable it locally:

```bash
git config core.hooksPath .githooks
```

Optional (`pre-commit` framework users): `.pre-commit-config.yaml` is also included.

## Embedded Niri E2E Test

An integration test is available at `crates/waybg-cli/tests/niri_embedded.rs` to validate rendering
in an embedded Niri session via screenshot pixel matching.

- It launches nested `niri`.
- Starts `waybg play` with a generated solid-color video.
- Captures a frame using `grim`.
- Verifies red/non-black pixel ratios.

Run it explicitly:

```bash
WAYSTREAM_E2E_NIRI=1 cargo test -p waybg-cli --test niri_embedded -- --nocapture
```

By default (`WAYSTREAM_E2E_NIRI` unset), this test is skipped.

## Xtask Commands

This repo provides Cargo aliases powered by `xtask`:

- `cargo ci`:
  runs formatter check, clippy, and normal tests (the same suite used in CI).
- `cargo normal-tests`:
  runs `cargo test --workspace --exclude xtask -- --skip niri_embedded_session_renders_video_pixels`.
- `cargo niri-tests`:
  runs `WAYSTREAM_E2E_NIRI=1 cargo test -p waybg-cli --test niri_embedded -- --nocapture`.

You can also run them directly through:

```bash
cargo xtask normal-tests
cargo xtask niri-tests
cargo xtask ci
```

## Release Flow

- On every push/merge to `main`, CI builds one Arch Linux ELF artifact:
  `waybg-<version>-archlinux-x86_64-elf`.
- The ELF is set executable with `chmod u+x`.
- A rolling prerelease named/tagged `test-main` is updated on every `main` merge.
- A versioned GitHub Release (`v<version>`) is published only when `crates/waybg-cli/Cargo.toml` `version` changes on `main`.
