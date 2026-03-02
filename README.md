# waystream

`waystream` is a Zig codebase for building an mpvpaper-style wallpaper/video background app with direct C-level API control.

## Dynamic Wayland Layer-Shell Background (Current)

The project now includes a Wayland/EGL layer-shell path in [`src/wayland_client.zig`](src/wayland_client.zig):

- Dynamically loads `libwayland-client.so` at runtime (no static Wayland link required)
- Links `EGL`, `wayland-egl`, and `GLESv2` as shared libraries at build time
- Connects to `wl_display`, discovers globals, binds `wl_compositor` and `zwlr_layer_shell_v1`
- Creates a fullscreen background `zwlr_layer_surface_v1` and draws an animated grayscale wave

Current runtime behavior:

- On start, `waystream` tries to run as a Wayland layer-shell background app.
- `waystream --video /path/to/file.mp4` (or `waystream /path/to/file.mp4`) plays video via `libmpv` with `audio=no` and `loop-file=inf`.
- `waystream --output DP-1 --video /path/to/file.mp4` targets a specific Wayland output name (connector).
- If no video path is provided, it renders the existing grayscale wave animation.
- If Wayland connection/setup fails, it exits with an explicit error.

This is the foundation step toward mpvpaper-style zero-copy video presentation on Wayland/Niri.

## Prerequisites

- Zig `0.15.2`
- `curl`, `tar`, `meson`, `ninja` (for native `libmpv` build)

## Developer Commands

Use `zig build` targets directly:

- `zig build` (debug build)
- `zig build run`
- `zig build test`
- `zig build fmt`
- `zig build fmt-check`
- `zig build ci`
- `zig build package -Doptimize=ReleaseSafe --prefix dist`
- `zig build package -Doptimize=ReleaseSafe --prefix dist -Drelease-version=0.1.0`
- `zig build musl-release --prefix dist`

By default, `package` uses `.version` from `build.zig.zon` for artifact naming. Use `-Drelease-version` to override.

## Native Static Build (`libmpv` + `mimalloc`)

`x86_64-linux-musl` builds can statically link:

- `libmpv` (from upstream tarball via Meson)
- `mimalloc` (from upstream tarball, compiled with `zig cc`)

Useful options:

- `-Dnative-deps=true`: enable native dependency build (defaults to true on `x86_64-linux-musl`)
- `-Dsystem-mpv=true`: link host/system `libmpv` dynamically (defaults to true on non-musl Linux)
- `-Dasan=true`: enable AddressSanitizer flags for native dependency builds
- `-Dlto=false`: disable executable LTO
- `-Dlibmpv-version=0.39.0`: set mpv version (URL defaults from this)
- `-Dlibmpv-tarball-url=...`: override mpv tarball URL
- `-Dlibmpv-extra-meson-args="..."`: pass extra Meson args to mpv setup
- `-Dmimalloc-version=v3.2.8`: set mimalloc tag
- `-Dmimalloc-tarball-url=...`: override mimalloc tarball URL

Example:

```sh
zig build musl-release --prefix dist
```

For normal Linux host builds, this is enough for video support:

```sh
zig build -Doptimize=ReleaseSafe
```

## CI and Release

GitHub Actions is Zig-native and uses `zig build` targets directly.
A tagged GitHub release is created only when `.version` in `build.zig.zon` changes.
