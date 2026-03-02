// raylib-zig (c) Nikolas Wipper 2023

const std = @import("std");
const native_c = @import("native_c.zig");
const wayland_client = @import("wayland_client.zig");
const build_options = @import("build_options");

pub fn main() anyerror!void {
    if (native_c.enabled) {
        std.debug.print(
            "libmpv api=0x{x} ({s}), mimalloc={d} ({s})\n",
            .{
                native_c.libmpvApiVersion(),
                build_options.libmpv_version,
                native_c.mimallocVersion(),
                build_options.mimalloc_version,
            },
        );
    }

    if (wayland_client.runGrayLayerShellAnimation()) {
        return;
    } else |err| {
        std.debug.print("wayland layer-shell background mode unavailable: {}\n", .{err});
        return err;
    }
}
