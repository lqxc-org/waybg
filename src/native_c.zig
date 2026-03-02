const std = @import("std");
const build_options = @import("build_options");

pub const enabled = build_options.has_mpv;
pub const has_mimalloc = build_options.has_mimalloc;

pub const c = if (enabled) @cImport({
    @cInclude("mpv/client.h");
    @cInclude("mpv/render.h");
    @cInclude("mpv/render_gl.h");
}) else struct {};

pub const mimalloc_c = if (has_mimalloc) @cImport({
    @cInclude("mimalloc.h");
}) else struct {};

extern fn eglGetProcAddress([*:0]const u8) callconv(.c) ?*const anyopaque;

pub const Error = error{
    NativeDepsDisabled,
    OutOfMemory,
    MpvCreateFailed,
    MpvOptionFailed,
    MpvLogConfigFailed,
    MpvInitializeFailed,
    MpvRenderContextCreateFailed,
    MpvCommandFailed,
    MpvContextMissing,
    MpvRenderFailed,
};

const mpv_impl = if (enabled) struct {
    fn errorString(status: c_int) []const u8 {
        const raw = c.mpv_error_string(status);
        if (raw == null) return "unknown";
        return std.mem.span(raw);
    }

    fn setOption(
        handle: *c.mpv_handle,
        name: [:0]const u8,
        value: [:0]const u8,
    ) Error!void {
        const status = c.mpv_set_option_string(handle, name.ptr, value.ptr);
        if (status < 0) {
            std.log.err("mpv set option {s}={s} failed: {s}", .{
                name,
                value,
                errorString(@as(c_int, @intCast(status))),
            });
            return error.MpvOptionFailed;
        }
    }

    fn loadFile(handle: *c.mpv_handle, video_path: []const u8) Error!void {
        const allocator = std.heap.c_allocator;
        const video_path_z = try allocator.dupeZ(u8, video_path);
        defer allocator.free(video_path_z);

        const command = [_:null]?[*:0]const u8{
            "loadfile",
            @as([*:0]const u8, video_path_z.ptr),
            "replace",
            null,
        };
        const status = c.mpv_command(handle, @ptrCast(@constCast(&command)));
        if (status < 0) {
            std.log.err("mpv loadfile failed for {s}: {s}", .{
                video_path,
                errorString(@as(c_int, @intCast(status))),
            });
            return error.MpvCommandFailed;
        }
    }

    fn setLogLevel(handle: *c.mpv_handle) Error!void {
        const status = c.mpv_request_log_messages(handle, "warn");
        if (status < 0) {
            std.log.err("mpv log setup failed: {s}", .{errorString(@as(c_int, @intCast(status)))});
            return error.MpvLogConfigFailed;
        }
    }
} else struct {};

pub const MpvRenderer = struct {
    handle: ?*anyopaque = null,
    render_context: ?*anyopaque = null,

    pub fn init(video_path: []const u8) Error!MpvRenderer {
        if (comptime enabled) {
            var renderer: MpvRenderer = .{};
            const handle = c.mpv_create() orelse return error.MpvCreateFailed;
            errdefer c.mpv_terminate_destroy(handle);

            try mpv_impl.setOption(handle, "vo", "libmpv");
            try mpv_impl.setOption(handle, "audio", "no");
            try mpv_impl.setOption(handle, "loop-file", "inf");
            try mpv_impl.setOption(handle, "hwdec", "auto-safe");

            if (c.mpv_initialize(handle) < 0) {
                return error.MpvInitializeFailed;
            }
            try mpv_impl.setLogLevel(handle);

            const GlInitParams = c.mpv_opengl_init_params;
            var gl_init_params: GlInitParams = std.mem.zeroes(GlInitParams);
            gl_init_params.get_proc_address = getProcAddress;
            gl_init_params.get_proc_address_ctx = null;
            if (comptime @hasField(GlInitParams, "extra_exts")) {
                gl_init_params.extra_exts = null;
            }
            var render_params = [_]c.mpv_render_param{
                .{
                    .type = c.MPV_RENDER_PARAM_API_TYPE,
                    .data = @ptrCast(@constCast(c.MPV_RENDER_API_TYPE_OPENGL)),
                },
                .{
                    .type = c.MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                    .data = @ptrCast(&gl_init_params),
                },
                .{
                    .type = c.MPV_RENDER_PARAM_INVALID,
                    .data = null,
                },
            };
            var render_context: ?*c.mpv_render_context = null;
            if (c.mpv_render_context_create(&render_context, handle, &render_params) < 0 or render_context == null) {
                return error.MpvRenderContextCreateFailed;
            }
            errdefer c.mpv_render_context_free(render_context.?);

            try mpv_impl.loadFile(handle, video_path);

            renderer.handle = @ptrCast(handle);
            renderer.render_context = @ptrCast(render_context.?);
            return renderer;
        } else {
            return error.NativeDepsDisabled;
        }
    }

    pub fn drainEvents(self: *MpvRenderer) bool {
        if (comptime enabled) {
            const handle_ptr = self.handle orelse return false;
            const handle: *c.mpv_handle = @ptrCast(@alignCast(handle_ptr));
            while (true) {
                const event = c.mpv_wait_event(handle, 0);
                if (event == null) return false;
                switch (event.*.event_id) {
                    c.MPV_EVENT_NONE => return false,
                    c.MPV_EVENT_SHUTDOWN => return true,
                    c.MPV_EVENT_LOG_MESSAGE => {
                        const log_msg: *const c.mpv_event_log_message = @ptrCast(@alignCast(event.*.data));
                        const prefix = if (log_msg.prefix != null) std.mem.span(log_msg.prefix) else "mpv";
                        const level = if (log_msg.level != null) std.mem.span(log_msg.level) else "unknown";
                        const text = if (log_msg.text != null) std.mem.trimRight(u8, std.mem.span(log_msg.text), "\n") else "";
                        std.log.warn("mpv[{s}/{s}] {s}", .{ prefix, level, text });
                    },
                    c.MPV_EVENT_END_FILE => {
                        const end_file: *const c.mpv_event_end_file = @ptrCast(@alignCast(event.*.data));
                        if (end_file.reason == c.MPV_END_FILE_REASON_ERROR) {
                            std.log.err("mpv playback ended with error: {s}", .{
                                mpv_impl.errorString(end_file.@"error"),
                            });
                            return true;
                        }
                    },
                    else => {},
                }
            }
        } else {
            return false;
        }
    }

    pub fn render(self: *MpvRenderer, width: i32, height: i32) Error!void {
        if (comptime enabled) {
            const render_ctx_ptr = self.render_context orelse return error.MpvContextMissing;
            const render_context: *c.mpv_render_context = @ptrCast(@alignCast(render_ctx_ptr));

            var fbo = c.mpv_opengl_fbo{
                .fbo = 0,
                .w = @as(c_int, @intCast(width)),
                .h = @as(c_int, @intCast(height)),
                .internal_format = 0,
            };
            var flip_y: c_int = 1;
            var render_params = [_]c.mpv_render_param{
                .{
                    .type = c.MPV_RENDER_PARAM_OPENGL_FBO,
                    .data = @ptrCast(&fbo),
                },
                .{
                    .type = c.MPV_RENDER_PARAM_FLIP_Y,
                    .data = @ptrCast(&flip_y),
                },
                .{
                    .type = c.MPV_RENDER_PARAM_INVALID,
                    .data = null,
                },
            };
            const status = c.mpv_render_context_render(render_context, &render_params);
            if (status < 0) {
                std.log.err("mpv render failed: {s}", .{mpv_impl.errorString(@as(c_int, @intCast(status)))});
                return error.MpvRenderFailed;
            }
        } else {
            return error.NativeDepsDisabled;
        }
    }

    pub fn deinit(self: *MpvRenderer) void {
        if (comptime enabled) {
            if (self.render_context) |render_ctx_ptr| {
                const render_context: *c.mpv_render_context = @ptrCast(@alignCast(render_ctx_ptr));
                c.mpv_render_context_free(render_context);
            }
            if (self.handle) |handle_ptr| {
                const handle: *c.mpv_handle = @ptrCast(@alignCast(handle_ptr));
                c.mpv_terminate_destroy(handle);
            }
        }
        self.handle = null;
        self.render_context = null;
    }
};

fn getProcAddress(_: ?*anyopaque, name: [*c]const u8) callconv(.c) ?*anyopaque {
    if (name == null) return null;
    const symbol = eglGetProcAddress(@ptrCast(name)) orelse return null;
    return @ptrCast(@constCast(symbol));
}

pub fn libmpvApiVersion() u32 {
    if (!enabled) return 0;
    return @as(u32, @intCast(c.mpv_client_api_version()));
}

pub fn mimallocVersion() i32 {
    if (!has_mimalloc) return 0;
    return @as(i32, @intCast(mimalloc_c.mi_version()));
}
