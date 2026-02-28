# waystream

`waystream` is a Zig + raylib codebase for building an mpvpaper-style wallpaper/video background app with direct C-level API control.

## Prerequisites

- Zig `0.15.2`
- `just` (optional, but recommended)

## Developer Commands

If you use `just`:

- `just fmt`: format Zig sources
- `just fmt-check`: fail if formatting is needed
- `just build`: debug build
- `just run`: run the app
- `just test`: run unit tests
- `just check`: run CI-equivalent checks
- `just release`: build release artifacts in `dist/`
- `just release-version 0.1.0`: override release artifact version suffix
- `just clean`: remove local build outputs

You can run the same targets directly with `zig build`:

- `zig build fmt`
- `zig build fmt-check`
- `zig build ci`
- `zig build package -Doptimize=ReleaseSafe --prefix dist`
- `zig build package -Doptimize=ReleaseSafe --prefix dist -Drelease-version=0.1.0`

By default, `package` uses `.version` from `build.zig.zon` for artifact naming. Use `-Drelease-version` to override.

## CI and Release

GitHub Actions is Zig-native and uses `zig build` targets directly.
A tagged GitHub release is created only when `.version` in `build.zig.zon` changes.
