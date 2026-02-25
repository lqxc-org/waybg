# Repository Guidelines

## Project Structure & Module Organization
This is a Rust workspace with focused crates under `crates/`:
- `waybg-cli`: user-facing `waybg` commands (`play`, `auto`, `gui`, `init-config`).
- `waybg-core`: shared config, profile, and scheduling logic.
- `waybg-daemon`: background scheduler/override runner.
- `waybg-ui`: Freya desktop UI for profile control and metrics.
- `wayland-core`: Wayland/layer-shell renderer and GStreamer playback engine.
- `xtask`: CI/dev task runner (`cargo ci`, `cargo normal-tests`, `cargo niri-tests`).

Unit tests usually live beside source (`src/*.rs` with `#[cfg(test)]`). Integration tests live in `crates/*/tests/` (for example `crates/waybg-cli/tests/niri_embedded.rs`).

## Build, Test, and Development Commands
- `cargo check --workspace`: fast compile validation across all crates.
- `cargo test`: run all unit, integration, and doc tests.
- `cargo fmt --all`: apply Rust formatting.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: lint with warnings treated as errors.
- `cargo ci`: run the CI-equivalent local suite.
- `cargo normal-tests`: run workspace tests excluding Niri embedded E2E.
- `cargo niri-tests`: run Niri embedded E2E only.
- `cargo run -p waybg-cli -- auto`: run auto profile switching locally.

## Coding Style & Naming Conventions
Use `rustfmt` defaults (4-space indentation). Follow standard Rust naming:
- `snake_case` for functions/modules/files.
- `PascalCase` for structs/enums/traits.
- `SCREAMING_SNAKE_CASE` for constants/env keys.

Prefer small, crate-local responsibilities: keep profile/schedule logic in `waybg-core`, renderer/pipeline code in `wayland-core`, and command orchestration in `waybg-cli`.

## Testing Guidelines
Add/extend tests with each behavior change:
- Parser and helper logic: unit tests near implementation.
- Cross-crate behavior and CLI flows: integration tests in `crates/<crate>/tests`.
- Niri-specific rendering behavior: cover via `niri_embedded` style tests when feasible.

Before opening a PR, run at least `cargo fmt --all`, `cargo clippy ... -D warnings`, and `cargo test`.

## Commit & Pull Request Guidelines
Commit history follows Conventional Commits, typically `type(scope): summary` (for example `feat(waybg): ...`).

For PRs, include:
- What changed and why.
- Linked issue(s) when applicable.
- Test evidence (commands run and results).
- UI/rendering proof (screenshot or short recording) for visible behavior changes.
