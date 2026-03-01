const std = @import("std");
const config = @import("config.zig");

pub const Context = struct {
    b: *std.Build,
    target: std.Build.ResolvedTarget,
    optimize: std.builtin.OptimizeMode,
    raylib: *std.Build.Module,
    has_native_deps: bool,
    libmpv_version: []const u8,
    mimalloc_version: []const u8,
};

pub fn createRootModule(ctx: Context) *std.Build.Module {
    const root_module = ctx.b.createModule(.{
        .root_source_file = ctx.b.path(config.root_source_file),
        .target = ctx.target,
        .optimize = ctx.optimize,
    });
    const build_options = ctx.b.addOptions();
    build_options.addOption(bool, "has_native_deps", ctx.has_native_deps);
    build_options.addOption([]const u8, "libmpv_version", ctx.libmpv_version);
    build_options.addOption([]const u8, "mimalloc_version", ctx.mimalloc_version);

    root_module.addImport("raylib", ctx.raylib);
    root_module.addOptions("build_options", build_options);
    return root_module;
}

pub fn createNativeExecutable(ctx: Context, root_module: *std.Build.Module) *std.Build.Step.Compile {
    return ctx.b.addExecutable(.{
        .name = config.app_name,
        .root_module = root_module,
    });
}

pub fn addUnitTests(ctx: Context) *std.Build.Step.Run {
    const test_module = createRootModule(.{
        .b = ctx.b,
        .target = ctx.b.graph.host,
        .optimize = ctx.optimize,
        .raylib = ctx.raylib,
        .has_native_deps = false,
        .libmpv_version = "disabled",
        .mimalloc_version = "disabled",
    });

    const unit_tests = ctx.b.addTest(.{
        .name = config.test_name,
        .root_module = test_module,
    });

    return ctx.b.addRunArtifact(unit_tests);
}
