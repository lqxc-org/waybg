const std = @import("std");
const config = @import("build/config.zig");
const app_build = @import("build/app.zig");
const build_steps = @import("build/steps.zig");
const metadata = @import("build/metadata.zig");
const native_deps = @import("build/native_deps.zig");

pub fn build(b: *std.Build) !void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const target_is_linux = target.result.os.tag == .linux;
    const target_is_musl_native = target_is_linux and target.result.abi == .musl and target.result.cpu.arch == .x86_64;
    const native_deps_enabled = b.option(
        bool,
        "native-deps",
        "Enable libmpv/mimalloc native dependency build (supported on x86_64-linux-musl)",
    ) orelse target_is_musl_native;
    const system_mpv_enabled = b.option(
        bool,
        "system-mpv",
        "Link system libmpv dynamically (default on non-musl Linux)",
    ) orelse (target_is_linux and !native_deps_enabled);
    if (native_deps_enabled and system_mpv_enabled) {
        std.log.err("choose only one: -Dnative-deps=true or -Dsystem-mpv=true", .{});
        return error.InvalidOption;
    }

    const default_release_version = metadata.readPackageVersion(b.allocator, b.pathFromRoot("build.zig.zon")) catch "dev";
    const release_version = b.option(
        []const u8,
        "release-version",
        "Release version for artifact naming (default: build.zig.zon .version)",
    ) orelse default_release_version;
    const asan = b.option(bool, "asan", "Enable AddressSanitizer for native dependencies") orelse false;
    const enable_lto = b.option(bool, "lto", "Enable link-time optimization for native executable") orelse (optimize != .Debug);
    const libmpv_version = b.option(
        []const u8,
        "libmpv-version",
        "mpv release version (without leading v)",
    ) orelse "0.39.0";
    const libmpv_tarball_url = b.option(
        []const u8,
        "libmpv-tarball-url",
        "Override libmpv tarball URL",
    ) orelse b.fmt("https://github.com/mpv-player/mpv/archive/refs/tags/v{s}.tar.gz", .{libmpv_version});
    const libmpv_extra_meson_args = b.option(
        []const u8,
        "libmpv-extra-meson-args",
        "Extra args passed to mpv meson setup (space-separated)",
    ) orelse "";
    const mimalloc_version = b.option(
        []const u8,
        "mimalloc-version",
        "mimalloc release tag",
    ) orelse "v3.2.8";
    const mimalloc_tarball_url = b.option(
        []const u8,
        "mimalloc-tarball-url",
        "Override mimalloc tarball URL",
    ) orelse b.fmt(
        "https://github.com/microsoft/mimalloc/archive/refs/tags/{s}.tar.gz",
        .{mimalloc_version},
    );
    const has_mpv = native_deps_enabled or system_mpv_enabled;
    const has_mimalloc = native_deps_enabled;
    const effective_libmpv_version = if (native_deps_enabled) libmpv_version else if (system_mpv_enabled) "system" else "disabled";
    const effective_mimalloc_version = if (native_deps_enabled) mimalloc_version else "disabled";

    const app_ctx: app_build.Context = .{
        .b = b,
        .target = target,
        .optimize = optimize,
        .has_mpv = false,
        .has_mimalloc = false,
        .libmpv_version = "disabled",
        .mimalloc_version = "disabled",
    };

    const run_step = b.step("run", "Run the app");
    const test_step = b.step("test", "Run unit tests on host");
    const run_unit_tests = app_build.addUnitTests(app_ctx);
    test_step.dependOn(&run_unit_tests.step);

    const fmt_steps = build_steps.addFmtSteps(b);
    _ = build_steps.addCiStep(b, fmt_steps.fmt_check, b.getInstallStep(), test_step);

    if (target.query.os_tag == .emscripten) {
        std.log.err("emscripten/web target is no longer supported", .{});
        return error.UnsupportedTarget;
    }

    const native_app_ctx: app_build.Context = .{
        .b = b,
        .target = target,
        .optimize = optimize,
        .has_mpv = has_mpv,
        .has_mimalloc = has_mimalloc,
        .libmpv_version = effective_libmpv_version,
        .mimalloc_version = effective_mimalloc_version,
    };
    const root_module = app_build.createRootModule(native_app_ctx);
    const exe = app_build.createNativeExecutable(native_app_ctx, root_module);
    configureExecutable(exe, has_mpv, enable_lto);

    if (native_deps_enabled) {
        if (!target_is_musl_native) {
            std.log.err(
                "native deps require -Dtarget=x86_64-linux-musl (or disable with -Dnative-deps=false)",
                .{},
            );
            return error.UnsupportedTarget;
        }

        const deps_options: native_deps.Options = .{
            .target = target,
            .optimize = optimize,
            .asan = asan,
            .mpv_version = libmpv_version,
            .mpv_tarball_url = libmpv_tarball_url,
            .mpv_extra_meson_args = libmpv_extra_meson_args,
            .mimalloc_version = mimalloc_version,
            .mimalloc_tarball_url = mimalloc_tarball_url,
        };
        try linkBundledDeps(b, exe, deps_options);
    } else if (system_mpv_enabled) {
        exe.root_module.linkSystemLibrary("mpv", .{
            .preferred_link_mode = .dynamic,
        });
    }

    b.installArtifact(exe);

    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());
    _ = build_steps.addPackageStep(b, exe, release_version);
    run_step.dependOn(&run_cmd.step);

    try addMuslReleaseStep(b, .{
        .release_version = release_version,
        .libmpv_version = libmpv_version,
        .libmpv_tarball_url = libmpv_tarball_url,
        .libmpv_extra_meson_args = libmpv_extra_meson_args,
        .mimalloc_version = mimalloc_version,
        .mimalloc_tarball_url = mimalloc_tarball_url,
        .asan = asan,
    });
}

const MuslReleaseOptions = struct {
    release_version: []const u8,
    libmpv_version: []const u8,
    libmpv_tarball_url: []const u8,
    libmpv_extra_meson_args: []const u8,
    mimalloc_version: []const u8,
    mimalloc_tarball_url: []const u8,
    asan: bool,
};

fn addMuslReleaseStep(b: *std.Build, options: MuslReleaseOptions) !void {
    const musl_release_step = b.step(
        "musl-release",
        "Build x86_64-linux-musl ReleaseSafe binary with bundled libmpv/mimalloc",
    );
    const musl_target = b.resolveTargetQuery(.{
        .cpu_arch = .x86_64,
        .os_tag = .linux,
        .abi = .musl,
    });
    const musl_optimize: std.builtin.OptimizeMode = .ReleaseSafe;
    const app_ctx: app_build.Context = .{
        .b = b,
        .target = musl_target,
        .optimize = musl_optimize,
        .has_mpv = true,
        .has_mimalloc = true,
        .libmpv_version = options.libmpv_version,
        .mimalloc_version = options.mimalloc_version,
    };

    const root_module = app_build.createRootModule(app_ctx);
    const exe = app_build.createNativeExecutable(app_ctx, root_module);
    configureExecutable(exe, true, true);

    const deps_options: native_deps.Options = .{
        .target = musl_target,
        .optimize = musl_optimize,
        .asan = options.asan,
        .mpv_version = options.libmpv_version,
        .mpv_tarball_url = options.libmpv_tarball_url,
        .mpv_extra_meson_args = options.libmpv_extra_meson_args,
        .mimalloc_version = options.mimalloc_version,
        .mimalloc_tarball_url = options.mimalloc_tarball_url,
    };
    try linkBundledDeps(b, exe, deps_options);

    const release_asset_name = b.fmt("{s}-{s}-linux-x86_64-musl", .{
        config.app_name,
        options.release_version,
    });
    const install_release = b.addInstallArtifact(exe, .{
        .dest_dir = .{ .override = .prefix },
        .dest_sub_path = release_asset_name,
    });
    musl_release_step.dependOn(&install_release.step);
}

fn configureExecutable(
    exe: *std.Build.Step.Compile,
    has_mpv: bool,
    enable_lto: bool,
) void {
    if (has_mpv) {
        exe.root_module.link_libc = true;
    }
    exe.root_module.linkSystemLibrary("EGL", .{
        .preferred_link_mode = .dynamic,
    });
    exe.root_module.linkSystemLibrary("wayland-egl", .{
        .preferred_link_mode = .dynamic,
    });
    exe.root_module.linkSystemLibrary("GLESv2", .{
        .preferred_link_mode = .dynamic,
    });
    if (enable_lto) {
        exe.want_lto = true;
    }
}

fn linkBundledDeps(
    b: *std.Build,
    exe: *std.Build.Step.Compile,
    options: native_deps.Options,
) !void {
    exe.linkage = .static;
    const artifacts = try native_deps.buildNativeDeps(b, options);
    exe.step.dependOn(artifacts.step);
    native_deps.linkNativeDeps(b, exe, artifacts);
    if (options.asan) {
        exe.root_module.linkSystemLibrary("asan", .{
            .use_pkg_config = .no,
            .preferred_link_mode = .static,
        });
    }
}
