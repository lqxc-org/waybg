const std = @import("std");
const rlz = @import("raylib_zig");
const config = @import("build/config.zig");
const app_build = @import("build/app.zig");
const build_steps = @import("build/steps.zig");
const metadata = @import("build/metadata.zig");

pub fn build(b: *std.Build) !void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const default_release_version = metadata.readPackageVersion(b.allocator, b.pathFromRoot("build.zig.zon")) catch "dev";
    const release_version = b.option(
        []const u8,
        "release-version",
        "Release version for artifact naming (default: build.zig.zon .version)",
    ) orelse default_release_version;

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
    };
    const root_module = app_build.createRootModule(app_ctx);

    const run_step = b.step("run", "Run the app");
    const test_step = b.step("test", "Run unit tests on host");
    const run_unit_tests = app_build.addUnitTests(app_ctx);
    test_step.dependOn(&run_unit_tests.step);

    const fmt_steps = build_steps.addFmtSteps(b);
    _ = build_steps.addCiStep(b, fmt_steps.fmt_check, b.getInstallStep(), test_step);

    // web exports are completely separate
    if (target.query.os_tag == .emscripten) {
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
        const exe = app_build.createNativeExecutable(app_ctx, root_module);
        b.installArtifact(exe);

        const run_cmd = b.addRunArtifact(exe);
        run_cmd.step.dependOn(b.getInstallStep());
        _ = build_steps.addPackageStep(b, exe, release_version);
        run_step.dependOn(&run_cmd.step);
    }
}
