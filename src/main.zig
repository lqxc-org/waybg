// raylib-zig (c) Nikolas Wipper 2023

const std = @import("std");
const rl = @import("raylib");
const native_c = @import("native_c.zig");
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

    // Initialization
    //--------------------------------------------------------------------------------------
    const screenWidth = 800;
    const screenHeight = 450;

    rl.initWindow(screenWidth, screenHeight, "raylib-zig [core] example - basic window");
    defer rl.closeWindow(); // Close window and OpenGL context

    rl.setTargetFPS(60); // Set our game to run at 60 frames-per-second
    //--------------------------------------------------------------------------------------

    // Main game loop
    while (!rl.windowShouldClose()) { // Detect window close button or ESC key
        // Update
        //----------------------------------------------------------------------------------
        // TODO: Update your variables here
        //----------------------------------------------------------------------------------

        // Draw
        //----------------------------------------------------------------------------------
        rl.beginDrawing();
        defer rl.endDrawing();

        rl.clearBackground(.white);

        rl.drawText("Congrats! You created your first window!", 190, 200, 20, .light_gray);
        //----------------------------------------------------------------------------------
    }
}
