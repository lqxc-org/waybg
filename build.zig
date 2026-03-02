const std = @import("std");
const rlz = @import("raylib_zig");
const config = @import("build/config.zig");
const app_build = @import("build/app.zig");
const build_steps = @import("build/steps.zig");
const metadata = @import("build/metadata.zig");
const native_deps = @import("build/native_deps.zig");

pub fn build(b: *std.Build) !void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const target_is_musl_native = target.result.os.tag == .linux and target.result.abi == .musl and target.result.cpu.arch == .x86_64;
    const native_deps_enabled = b.option(
        bool,
        "native-deps",
        "Enable libmpv/mimalloc native dependency build (supported on x86_64-linux-musl)",
    ) orelse target_is_musl_native;
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

    const raylib_dep = b.dependency("raylib_zig", .{
        .target = target,
        .optimize = optimize,
    });
    const raylib = raylib_dep.module("raylib");
    const raylib_artifact = raylib_dep.artifact("raylib");

    const app_ctx: app_build.Context = .{
        .b = b,
        .target = target,
        .optimize = optimize,
        .raylib = raylib,
        .has_native_deps = false,
        .libmpv_version = libmpv_version,
        .mimalloc_version = mimalloc_version,
    };

    const run_step = b.step("run", "Run the app");
    const test_step = b.step("test", "Run unit tests on host");
    const run_unit_tests = app_build.addUnitTests(app_ctx);
    test_step.dependOn(&run_unit_tests.step);

    const fmt_steps = build_steps.addFmtSteps(b);
    _ = build_steps.addCiStep(b, fmt_steps.fmt_check, b.getInstallStep(), test_step);

    // web exports are completely separate
    if (target.query.os_tag == .emscripten) {
        const root_module = app_build.createRootModule(.{
            .b = b,
            .target = target,
            .optimize = optimize,
            .raylib = raylib,
            .has_native_deps = false,
            .libmpv_version = "disabled",
            .mimalloc_version = "disabled",
        });

        const emsdk = rlz.emsdk;
        const wasm = b.addLibrary(.{
            .name = config.app_name,
            .root_module = root_module,
        });

        const install_dir: std.Build.InstallDir = .{ .custom = "web" };
        const emcc_flags = emsdk.emccDefaultFlags(b.allocator, .{ .optimize = optimize });
        const emcc_settings = emsdk.emccDefaultSettings(b.allocator, .{ .optimize = optimize });

        const emcc_step = emsdk.emccStep(b, raylib_artifact, wasm, .{
            .optimize = optimize,
            .flags = emcc_flags,
            .settings = emcc_settings,
            .shell_file_path = emsdk.shell(raylib_dep.builder),
            .install_dir = install_dir,
            .embed_paths = &.{.{ .src_path = "resources/" }},
        });
        b.getInstallStep().dependOn(emcc_step);

        const html_filename = try std.fmt.allocPrint(b.allocator, "{s}.html", .{wasm.name});
        const emrun_step = emsdk.emrunStep(
            b,
            b.getInstallPath(install_dir, html_filename),
            &.{},
        );

        emrun_step.dependOn(emcc_step);
        run_step.dependOn(emrun_step);
    } else {
        const native_app_ctx: app_build.Context = .{
            .b = b,
            .target = target,
            .optimize = optimize,
            .raylib = raylib,
            .has_native_deps = native_deps_enabled,
            .libmpv_version = libmpv_version,
            .mimalloc_version = mimalloc_version,
        };
        const root_module = app_build.createRootModule(native_app_ctx);
        const exe = app_build.createNativeExecutable(native_app_ctx, root_module);
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

        if (native_deps_enabled) {
            if (!target_is_musl_native) {
                std.log.err(
                    "native deps require -Dtarget=x86_64-linux-musl (or disable with -Dnative-deps=false)",
                    .{},
                );
                return error.UnsupportedTarget;
            }

            exe.linkage = .static;
            const artifacts = try native_deps.buildNativeDeps(b, .{
                .target = target,
                .optimize = optimize,
                .asan = asan,
                .mpv_version = libmpv_version,
                .mpv_tarball_url = libmpv_tarball_url,
                .mpv_extra_meson_args = libmpv_extra_meson_args,
                .mimalloc_version = mimalloc_version,
                .mimalloc_tarball_url = mimalloc_tarball_url,
            });
            exe.step.dependOn(artifacts.step);
            native_deps.linkNativeDeps(b, exe, artifacts);
            if (asan) {
                exe.root_module.linkSystemLibrary("asan", .{
                    .use_pkg_config = .no,
                    .preferred_link_mode = .static,
                });
            }
        }

        b.installArtifact(exe);

        const run_cmd = b.addRunArtifact(exe);
        run_cmd.step.dependOn(b.getInstallStep());
        _ = build_steps.addPackageStep(b, exe, release_version);
        run_step.dependOn(&run_cmd.step);
    }
}
