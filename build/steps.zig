const std = @import("std");
const config = @import("config.zig");

pub const FmtSteps = struct {
    fmt: *std.Build.Step,
    fmt_check: *std.Build.Step,
};

pub fn addFmtSteps(b: *std.Build) FmtSteps {
    const format_apply = b.addFmt(.{
        .paths = &config.fmt_paths,
    });
    const format_check = b.addFmt(.{
        .paths = &config.fmt_paths,
        .check = true,
    });

    const fmt_step = b.step("fmt", "Format Zig sources");
    fmt_step.dependOn(&format_apply.step);

    const fmt_check_step = b.step("fmt-check", "Check source formatting");
    fmt_check_step.dependOn(&format_check.step);

    return .{
        .fmt = fmt_step,
        .fmt_check = fmt_check_step,
    };
}

pub fn addCiStep(
    b: *std.Build,
    fmt_check: *std.Build.Step,
    install_step: *std.Build.Step,
    test_step: *std.Build.Step,
) *std.Build.Step {
    const ci_step = b.step("ci", "Run formatting checks, build, and tests");
    ci_step.dependOn(fmt_check);
    ci_step.dependOn(install_step);
    ci_step.dependOn(test_step);
    return ci_step;
}

pub fn addPackageStep(
    b: *std.Build,
    exe: *std.Build.Step.Compile,
    release_version: []const u8,
) *std.Build.Step {
    const release_asset_name = b.fmt("{s}-{s}-linux-x86_64-elf", .{ config.app_name, release_version });

    const install_release = b.addInstallArtifact(exe, .{
        .dest_dir = .{ .override = .prefix },
        .dest_sub_path = release_asset_name,
    });

    const release_abs_path = b.getInstallPath(.prefix, release_asset_name);
    const checksum_cmd = b.addSystemCommand(&.{
        "sha256sum",
        release_abs_path,
    });
    checksum_cmd.step.dependOn(&install_release.step);

    const install_checksum = b.addInstallFileWithDir(
        checksum_cmd.captureStdOut(),
        .prefix,
        "SHA256SUMS.txt",
    );

    const package_step = b.step("package", "Create release artifact and SHA256SUMS under --prefix");
    package_step.dependOn(&install_release.step);
    package_step.dependOn(&install_checksum.step);

    return package_step;
}
