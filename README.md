# waybg

Wayland/Niri video wallpaper controller built with:

- `smithay-client-toolkit` for Wayland session connectivity,
- `gstreamer` (`playbin` + `waylandsink`) for playback,
- `freya` for a native GUI profile preview/controller.

## Features

- `play`: direct playback on Wayland.
- `auto`: automatic profile-based wallpaper switching (time schedules + manual override).
- `gui`: Freya desktop UI to preview profiles and set/clear manual override.
- `init-config`: generate a starter `profiles.toml`.

## System dependencies

Install GStreamer runtime and Wayland sink support:

```bash
sudo apt install \
  libgstreamer1.0-dev \
  libgstreamer-plugins-base1.0-dev \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-libav
```

## Quick start

1) Generate sample config:

```bash
cargo run -- init-config --output profiles.toml
```

2) Edit `profiles.toml` with real video file paths.

3) Start automatic switching:

```bash
cargo run -- auto --config profiles.toml
```

4) Open Freya profile controller (optional, in another terminal):

```bash
cargo run -- gui --config profiles.toml
```

5) Direct one-off playback:

```bash
cargo run -- play /path/to/video.mp4 --loop-playback
```

## Config format

```toml
[settings]
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
```

`weekdays` uses ISO numbering: `1=Mon ... 7=Sun`.

## Pre-commit checks

This repo includes a Git pre-commit hook in `.githooks/pre-commit` that runs:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Enable it locally:

```bash
git config core.hooksPath .githooks
```

Optional (`pre-commit` framework users): `.pre-commit-config.yaml` is also included.

## Embedded Niri E2E Test

An integration test is available at `tests/niri_embedded.rs` to validate rendering in an embedded
Niri session via screenshot pixel matching.

- It launches nested `niri`.
- Starts `waybg play` with a generated solid-color video.
- Captures a frame using `grim`.
- Verifies red/non-black pixel ratios.

Run it explicitly:

```bash
WAYSTREAM_E2E_NIRI=1 cargo test --test niri_embedded -- --nocapture
```

By default (`WAYSTREAM_E2E_NIRI` unset), this test is skipped.

## Xtask Test Commands

This repo provides Cargo aliases powered by `xtask`:

- `cargo ci`:
  runs formatter check, clippy, and normal tests (the same suite used in CI).
- `cargo normal-tests`:
  runs `cargo test --workspace --exclude xtask -- --skip niri_embedded_session_renders_video_pixels`.
- `cargo niri-tests`:
  runs `WAYSTREAM_E2E_NIRI=1 cargo test --test niri_embedded -- --nocapture`.

You can also run them directly through:

```bash
cargo xtask normal-tests
cargo xtask niri-tests
cargo xtask ci
```
