# waystream

`waystream` is a Zig + raylib codebase for building an mpvpaper-style wallpaper/video background app with direct C-level API control.

## Dynamic Wayland Layer-Shell Background (Current)

The project now includes a Wayland/EGL layer-shell path in [`src/wayland_client.zig`](src/wayland_client.zig):

- Dynamically loads `libwayland-client.so` at runtime (no static Wayland link required)
- Links `EGL`, `wayland-egl`, and `GLESv2` as shared libraries at build time
- Connects to `wl_display`, discovers globals, binds `wl_compositor` and `zwlr_layer_shell_v1`
- Creates a fullscreen background `zwlr_layer_surface_v1` and draws an animated grayscale wave

Current runtime behavior:

- On start, `waystream` tries to run as a Wayland layer-shell background app.
- If Wayland connection/setup fails, it exits with an explicit error.

This is the foundation step toward mpvpaper-style zero-copy video presentation on Wayland/Niri.

## Prerequisites

- Zig `0.15.2`
- `just` (optional, but recommended)
- `curl`, `tar`, `meson`, `ninja` (for native `libmpv` build)

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

## Native Static Build (`libmpv` + `mimalloc`)

`x86_64-linux-musl` builds can statically link:

- `libmpv` (from upstream tarball via Meson)
- `mimalloc` (from upstream tarball, compiled with `zig cc`)

Useful options:

- `-Dnative-deps=true`: enable native dependency build (defaults to true on `x86_64-linux-musl`)
- `-Dasan=true`: enable AddressSanitizer flags for native dependency builds
- `-Dlto=false`: disable executable LTO
- `-Dlibmpv-version=0.39.0`: set mpv version (URL defaults from this)
- `-Dlibmpv-tarball-url=...`: override mpv tarball URL
- `-Dlibmpv-extra-meson-args="..."`: pass extra Meson args to mpv setup
- `-Dmimalloc-version=v3.2.8`: set mimalloc tag
- `-Dmimalloc-tarball-url=...`: override mimalloc tarball URL

Example:

```sh
zig build -Dtarget=x86_64-linux-musl -Dnative-deps=true -Doptimize=ReleaseFast
```

## CI and Release

GitHub Actions is Zig-native and uses `zig build` targets directly.
A tagged GitHub release is created only when `.version` in `build.zig.zon` changes.
