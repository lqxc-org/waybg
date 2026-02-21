# waybg

Wayland/Niri video wallpaper monorepo.

## Workspace Layout

- `crates/waybg-cli`: user-facing `waybg` command (`play`, `auto`, `gui`, `init-config`).
- `crates/waybg-ui`: Freya profile controller UI crate/binary.
- `crates/waybg-daemon`: schedule/override daemon crate/binary.
- `crates/wayland-core`: Wayland playback core (GStreamer decode + `waylandsink`).
- `crates/waybg-core`: shared profile/config/override scheduling logic.
- `xtask`: CI/dev task runner (`cargo ci`, `cargo normal-tests`, `cargo niri-tests`).

## Features

- `play`: direct playback on Wayland.
- `auto`: automatic profile-based wallpaper switching (time schedules + manual override).
- `gui`: Freya desktop UI to preview profiles and set/clear manual override.
- `init-config`: generate a starter `profiles.toml`.

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
  gst-libav
```

## Quick Start

1) Generate sample config:

```bash
cargo run -p waybg-cli -- init-config --output profiles.toml
```

2) Edit `profiles.toml` with real video file paths.

3) Start automatic switching:

```bash
cargo run -p waybg-cli -- auto --config profiles.toml
```

4) Open Freya profile controller (optional, in another terminal):

```bash
cargo run -p waybg-cli -- gui --config profiles.toml
```

5) Direct one-off playback:

```bash
cargo run -p waybg-cli -- play /path/to/video.mp4 --loop-playback
```

## Optional Standalone Binaries

Run daemon directly:

```bash
cargo run -p waybg-daemon -- run --config profiles.toml
```

Run UI directly:

```bash
cargo run -p waybg-ui -- run --config profiles.toml
```

## Config Format

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
