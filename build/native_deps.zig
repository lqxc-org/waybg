const std = @import("std");

pub const Options = struct {
    target: std.Build.ResolvedTarget,
    optimize: std.builtin.OptimizeMode,
    asan: bool,
    mpv_version: []const u8,
    mpv_tarball_url: []const u8,
    mpv_extra_meson_args: []const u8,
    mimalloc_version: []const u8,
    mimalloc_tarball_url: []const u8,
};

pub const Artifacts = struct {
    step: *std.Build.Step,
    mpv_install_dir: std.Build.LazyPath,
    mimalloc_install_dir: std.Build.LazyPath,

    pub fn mpvInclude(self: Artifacts, b: *std.Build) std.Build.LazyPath {
        return self.mpv_install_dir.path(b, "include");
    }

    pub fn mimallocInclude(self: Artifacts, b: *std.Build) std.Build.LazyPath {
        return self.mimalloc_install_dir.path(b, "include");
    }

    pub fn mpvStaticLib(self: Artifacts, b: *std.Build) std.Build.LazyPath {
        return self.mpv_install_dir.path(b, "lib/libmpv.a");
    }

    pub fn mimallocStaticLib(self: Artifacts, b: *std.Build) std.Build.LazyPath {
        return self.mimalloc_install_dir.path(b, "lib/libmimalloc.a");
    }
};

pub fn buildNativeDeps(b: *std.Build, options: Options) !Artifacts {
    const target = options.target.result;
    if (target.os.tag != .linux or target.abi != .musl or target.cpu.arch != .x86_64) {
        return error.UnsupportedTarget;
    }

    std.log.info("native deps: libmpv {s}, mimalloc {s}", .{
        options.mpv_version,
        options.mimalloc_version,
    });

    const buildtype: []const u8 = switch (options.optimize) {
        .Debug => "debug",
        .ReleaseSafe, .ReleaseFast => "release",
        .ReleaseSmall => "minsize",
    };
    const opt_level: []const u8 = switch (options.optimize) {
        .Debug => "-O0",
        .ReleaseSafe => "-O2",
        .ReleaseFast => "-O3",
        .ReleaseSmall => "-Os",
    };
    const asan_flag = if (options.asan) "1" else "0";

    const script =
        \\set -o pipefail
        \\
        \\work_dir="$1"
        \\mpv_prefix="$2"
        \\mimalloc_prefix="$3"
        \\stamp="$4"
        \\
        \\fetch_tarball() {
        \\  local url="$1"
        \\  local out="$2"
        \\  if [ ! -f "$out" ]; then
        \\    curl -fsSL "$url" -o "$out"
        \\  fi
        \\}
        \\
        \\extract_tarball() {
        \\  local tarball="$1"
        \\  local out_dir="$2"
        \\  local tmp_dir="${out_dir}.tmp"
        \\  rm -rf "$tmp_dir"
        \\  mkdir -p "$tmp_dir"
        \\  tar -xzf "$tarball" --strip-components=1 -C "$tmp_dir"
        \\  rm -rf "$out_dir"
        \\  mv "$tmp_dir" "$out_dir"
        \\}
        \\
        \\mkdir -p "$work_dir" "$mpv_prefix" "$mimalloc_prefix"
        \\
        \\mpv_tar="$work_dir/mpv-${WAYSTREAM_MPV_VERSION}.tar.gz"
        \\mpv_src="$work_dir/mpv-src"
        \\mpv_build="$work_dir/mpv-build"
        \\
        \\mimalloc_tar="$work_dir/mimalloc-${WAYSTREAM_MIMALLOC_VERSION}.tar.gz"
        \\mimalloc_src="$work_dir/mimalloc-src"
        \\mimalloc_build="$work_dir/mimalloc-build"
        \\
        \\fetch_tarball "$WAYSTREAM_MPV_TARBALL_URL" "$mpv_tar"
        \\fetch_tarball "$WAYSTREAM_MIMALLOC_TARBALL_URL" "$mimalloc_tar"
        \\
        \\extract_tarball "$mpv_tar" "$mpv_src"
        \\extract_tarball "$mimalloc_tar" "$mimalloc_src"
        \\
        \\cross_file="$work_dir/meson-cross-x86_64-linux-musl.ini"
        \\cat >"$cross_file" <<EOF
        \\[binaries]
        \\c = ['${WAYSTREAM_ZIG_EXE}', 'cc', '-target', 'x86_64-linux-musl']
        \\cpp = ['${WAYSTREAM_ZIG_EXE}', 'c++', '-target', 'x86_64-linux-musl']
        \\ar = ['${WAYSTREAM_ZIG_EXE}', 'ar']
        \\strip = ['${WAYSTREAM_ZIG_EXE}', 'strip']
        \\pkgconfig = 'pkg-config'
        \\
        \\[host_machine]
        \\system = 'linux'
        \\cpu_family = 'x86_64'
        \\cpu = 'x86_64'
        \\endian = 'little'
        \\
        \\[properties]
        \\needs_exe_wrapper = false
        \\c_args = ['-fPIC']
        \\cpp_args = ['-fPIC']
        \\c_link_args = ['-static']
        \\cpp_link_args = ['-static']
        \\EOF
        \\
        \\rm -rf "$mimalloc_build" "$mimalloc_prefix"
        \\mkdir -p "$mimalloc_build" "$mimalloc_prefix/lib" "$mimalloc_prefix/include"
        \\
        \\mimalloc_cc=("${WAYSTREAM_ZIG_EXE}" cc -target x86_64-linux-musl "$WAYSTREAM_OPT_LEVEL" -fPIC -DMI_MALLOC_OVERRIDE=1 -DMI_STATIC_LIB=1 -I"$mimalloc_src/include" -I"$mimalloc_src/src")
        \\if [ "$WAYSTREAM_ASAN" = "1" ]; then
        \\  mimalloc_cc+=(-fsanitize=address)
        \\fi
        \\mimalloc_cc+=(-c "$mimalloc_src/src/static.c" -o "$mimalloc_build/static.o")
        \\"${mimalloc_cc[@]}"
        \\
        \\"${WAYSTREAM_ZIG_EXE}" ar rcs "$mimalloc_prefix/lib/libmimalloc.a" "$mimalloc_build/static.o"
        \\cp -a "$mimalloc_src/include/." "$mimalloc_prefix/include/"
        \\
        \\rm -rf "$mpv_build" "$mpv_prefix"
        \\mkdir -p "$mpv_prefix"
        \\meson_setup_args=(
        \\  "$mpv_build"
        \\  "$mpv_src"
        \\  "--prefix" "$mpv_prefix"
        \\  "--buildtype" "$WAYSTREAM_MESON_BUILDTYPE"
        \\  "--default-library" "static"
        \\  "--cross-file" "$cross_file"
        \\  "-Dlibmpv=true"
        \\  "-Dcplayer=false"
        \\)
        \\if [ "$WAYSTREAM_ASAN" = "1" ]; then
        \\  meson_setup_args+=("-Db_sanitize=address")
        \\fi
        \\if [ -n "${WAYSTREAM_MPV_EXTRA_MESON_ARGS}" ]; then
        \\  # Intentionally split for additional meson args.
        \\  # shellcheck disable=SC2206
        \\  extra_args=( ${WAYSTREAM_MPV_EXTRA_MESON_ARGS} )
        \\  meson_setup_args+=("${extra_args[@]}")
        \\fi
        \\meson setup "${meson_setup_args[@]}"
        \\
        \\meson compile -C "$mpv_build"
        \\meson install -C "$mpv_build"
        \\
        \\if [ ! -f "$mpv_prefix/lib/libmpv.a" ]; then
        \\  echo "libmpv.a was not installed to $mpv_prefix/lib" >&2
        \\  exit 1
        \\fi
        \\if [ ! -f "$mimalloc_prefix/lib/libmimalloc.a" ]; then
        \\  echo "libmimalloc.a was not installed to $mimalloc_prefix/lib" >&2
        \\  exit 1
        \\fi
        \\
        \\printf 'libmpv=%s\nmimalloc=%s\n' "$WAYSTREAM_MPV_VERSION" "$WAYSTREAM_MIMALLOC_VERSION" >"$stamp"
    ;

    const cmd = b.addSystemCommand(&.{ "bash", "-uec", script, "--" });
    const work_dir = cmd.addOutputDirectoryArg("native-deps-work");
    const mpv_install_dir = cmd.addOutputDirectoryArg("native-deps-mpv");
    const mimalloc_install_dir = cmd.addOutputDirectoryArg("native-deps-mimalloc");
    _ = cmd.addOutputFileArg("native-deps.stamp");

    cmd.setEnvironmentVariable("WAYSTREAM_ZIG_EXE", b.graph.zig_exe);
    cmd.setEnvironmentVariable("WAYSTREAM_MPV_VERSION", options.mpv_version);
    cmd.setEnvironmentVariable("WAYSTREAM_MPV_TARBALL_URL", options.mpv_tarball_url);
    cmd.setEnvironmentVariable("WAYSTREAM_MPV_EXTRA_MESON_ARGS", options.mpv_extra_meson_args);
    cmd.setEnvironmentVariable("WAYSTREAM_MIMALLOC_VERSION", options.mimalloc_version);
    cmd.setEnvironmentVariable("WAYSTREAM_MIMALLOC_TARBALL_URL", options.mimalloc_tarball_url);
    cmd.setEnvironmentVariable("WAYSTREAM_OPT_LEVEL", opt_level);
    cmd.setEnvironmentVariable("WAYSTREAM_MESON_BUILDTYPE", buildtype);
    cmd.setEnvironmentVariable("WAYSTREAM_ASAN", asan_flag);

    _ = work_dir;

    return .{
        .step = &cmd.step,
        .mpv_install_dir = mpv_install_dir,
        .mimalloc_install_dir = mimalloc_install_dir,
    };
}

pub fn linkNativeDeps(
    b: *std.Build,
    exe: *std.Build.Step.Compile,
    artifacts: Artifacts,
) void {
    const root_module = exe.root_module;
    root_module.link_libc = true;
    root_module.addIncludePath(artifacts.mpvInclude(b));
    root_module.addIncludePath(artifacts.mimallocInclude(b));
    root_module.addObjectFile(artifacts.mpvStaticLib(b));
    root_module.addObjectFile(artifacts.mimallocStaticLib(b));

    root_module.linkSystemLibrary("m", .{
        .use_pkg_config = .no,
        .preferred_link_mode = .static,
    });
    root_module.linkSystemLibrary("dl", .{
        .use_pkg_config = .no,
        .preferred_link_mode = .static,
    });
    root_module.linkSystemLibrary("pthread", .{
        .use_pkg_config = .no,
        .preferred_link_mode = .static,
    });
}
