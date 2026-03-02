const std = @import("std");
const native_c = @import("native_c.zig");
const wayland_client = @import("wayland_client.zig");
const build_options = @import("build_options");

const CliError = error{
    HelpRequested,
    MissingVideoPath,
    MissingOutputName,
    InvalidArgument,
};

const CliOptions = struct {
    video_path: ?[]const u8 = null,
    output_name: ?[]const u8 = null,
};

pub fn main() anyerror!void {
    const cli_options = parseCliArgs() catch |err| {
        switch (err) {
            error.HelpRequested => {
                printUsage();
                return;
            },
            error.MissingVideoPath => {
                std.debug.print("missing value for --video\n", .{});
                printUsage();
            },
            error.MissingOutputName => {
                std.debug.print("missing value for --output\n", .{});
                printUsage();
            },
            error.InvalidArgument => {
                printUsage();
            },
        }
        return err;
    };

    if (cli_options.video_path != null and !native_c.enabled) {
        std.debug.print(
            "video playback requires libmpv support in this build\n",
            .{},
        );
        std.debug.print("try: zig build -Dsystem-mpv=true -Doptimize=ReleaseSafe\n", .{});
        std.debug.print("or static: zig build -Dtarget=x86_64-linux-musl -Dnative-deps=true -Doptimize=ReleaseSafe\n", .{});
        return error.NativeDepsDisabled;
    }

    if (native_c.enabled) {
        std.debug.print("libmpv api=0x{x} ({s})\n", .{
            native_c.libmpvApiVersion(),
            build_options.libmpv_version,
        });
        if (native_c.has_mimalloc) {
            std.debug.print("mimalloc={d} ({s})\n", .{
                native_c.mimallocVersion(),
                build_options.mimalloc_version,
            });
        }
    }

    if (wayland_client.runLayerShellBackground(.{
        .video_path = cli_options.video_path,
        .output_name = cli_options.output_name,
    })) {
        return;
    } else |err| {
        std.debug.print("wayland layer-shell background mode unavailable: {}\n", .{err});
        return err;
    }
}

fn parseCliArgs() CliError!CliOptions {
    var options: CliOptions = .{};
    var args = std.process.args();
    _ = args.next();

    while (args.next()) |arg| {
        if (std.mem.eql(u8, arg, "-h") or std.mem.eql(u8, arg, "--help")) {
            return error.HelpRequested;
        }
        if (std.mem.eql(u8, arg, "--video")) {
            const video_path = args.next() orelse return error.MissingVideoPath;
            options.video_path = video_path;
            continue;
        }
        if (std.mem.startsWith(u8, arg, "--video=")) {
            const video_path = arg["--video=".len..];
            if (video_path.len == 0) return error.MissingVideoPath;
            options.video_path = video_path;
            continue;
        }
        if (std.mem.eql(u8, arg, "--output") or std.mem.eql(u8, arg, "-o")) {
            const output_name = args.next() orelse return error.MissingOutputName;
            options.output_name = output_name;
            continue;
        }
        if (std.mem.startsWith(u8, arg, "--output=")) {
            const output_name = arg["--output=".len..];
            if (output_name.len == 0) return error.MissingOutputName;
            options.output_name = output_name;
            continue;
        }

        // mpvpaper compatibility flags accepted as no-ops.
        if (std.mem.eql(u8, arg, "-v") or std.mem.eql(u8, arg, "-s") or std.mem.eql(u8, arg, "-vs") or std.mem.eql(u8, arg, "-sv")) {
            continue;
        }

        if (std.mem.startsWith(u8, arg, "-")) {
            std.debug.print("invalid argument: {s}\n", .{arg});
            return error.InvalidArgument;
        }

        if (options.video_path != null) {
            std.debug.print("extra positional argument: {s}\n", .{arg});
            return error.InvalidArgument;
        }
        options.video_path = arg;
    }

    return options;
}

fn printUsage() void {
    std.debug.print(
        \\Usage: waystream [--video <path>]
        \\       waystream [--output <name>] [<video-path>]
        \\
        \\Options:
        \\  --video <path>        Video file to play on the background (audio disabled, loop enabled)
        \\  --output, -o <name>   Target Wayland output name (for example DP-1)
        \\  -v -s -vs -sv         Accepted mpvpaper compatibility no-op flags
        \\  --help, -h           Show this help text
        \\
    , .{});
}
