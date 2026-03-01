const build_options = @import("build_options");

pub const enabled = build_options.has_native_deps;

pub const c = if (enabled) @cImport({
    @cInclude("mpv/client.h");
    @cInclude("mimalloc.h");
}) else struct {};

pub fn libmpvApiVersion() u32 {
    if (!enabled) return 0;
    return @as(u32, @intCast(c.mpv_client_api_version()));
}

pub fn mimallocVersion() i32 {
    if (!enabled) return 0;
    return @as(i32, @intCast(c.mi_version()));
}
