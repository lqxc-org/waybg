const std = @import("std");
const builtin = @import("builtin");

const log = std.log.scoped(.wayland);

pub const wl_proxy = opaque {};
pub const wl_display = opaque {};
pub const wl_registry = opaque {};
pub const wl_compositor = opaque {};
pub const wl_surface = opaque {};
pub const wl_output = opaque {};
pub const wl_egl_window = opaque {};
pub const zwlr_layer_shell_v1 = opaque {};
pub const zwlr_layer_surface_v1 = opaque {};

pub const wl_message = extern struct {
    name: [*:0]const u8,
    signature: [*:0]const u8,
    types: ?[*]const ?*const wl_interface,
};

pub const wl_interface = extern struct {
    name: [*:0]const u8,
    version: c_int,
    method_count: c_int,
    methods: ?[*]const wl_message,
    event_count: c_int,
    events: ?[*]const wl_message,
};

pub const wl_registry_listener = extern struct {
    global: ?*const fn (
        data: ?*anyopaque,
        registry: *wl_registry,
        name: u32,
        interface: [*:0]const u8,
        version: u32,
    ) callconv(.c) void,
    global_remove: ?*const fn (
        data: ?*anyopaque,
        registry: *wl_registry,
        name: u32,
    ) callconv(.c) void,
};

pub const Global = struct {
    name: u32,
    version: u32,
};

pub const Globals = struct {
    compositor: ?Global = null,
    shm: ?Global = null,
    xdg_wm_base: ?Global = null,
    layer_shell: ?Global = null,
};

pub const EglVersion = struct {
    major: i32,
    minor: i32,
};

pub const Error = error{
    UnsupportedOS,
    WaylandLibNotFound,
    WaylandSymbolMissing,
    DisplayConnectFailed,
    RegistryCreateFailed,
    RegistryListenerFailed,
    RoundtripFailed,
    MissingCompositor,
    BindCompositorFailed,
    BindLayerShellFailed,
    SurfaceCreateFailed,
    MissingLayerShell,
    LayerSurfaceCreateFailed,
    LayerSurfaceListenerFailed,
    LayerSurfaceConfigureFailed,
    LayerSurfaceClosed,
    EglDisplayInitFailed,
    EglConfigFailed,
    EglContextCreateFailed,
    EglWindowSurfaceCreateFailed,
    EglMakeCurrentFailed,
    EglSwapFailed,
    WlEglWindowCreateFailed,
    DispatchFailed,
};

const WL_DISPLAY_GET_REGISTRY: u32 = 1;
const WL_REGISTRY_BIND: u32 = 0;
const WL_COMPOSITOR_CREATE_SURFACE: u32 = 0;
const WL_SURFACE_COMMIT: u32 = 6;
const WL_MARSHAL_FLAG_DESTROY: u32 = 1;

const EGL_FALSE: u32 = 0;
const EGL_PLATFORM_WAYLAND_KHR: u32 = 0x31D8;
const EGL_NONE: i32 = 0x3038;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_WINDOW_BIT: i32 = 0x0004;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_ALPHA_SIZE: i32 = 0x3021;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const EGL_OPENGL_ES_API: u32 = 0x30A0;

const GL_COLOR_BUFFER_BIT: u32 = 0x00004000;

const ZWLR_LAYER_SHELL_V1_GET_LAYER_SURFACE: u32 = 0;
const ZWLR_LAYER_SURFACE_V1_SET_SIZE: u32 = 0;
const ZWLR_LAYER_SURFACE_V1_SET_ANCHOR: u32 = 1;
const ZWLR_LAYER_SURFACE_V1_SET_EXCLUSIVE_ZONE: u32 = 2;
const ZWLR_LAYER_SURFACE_V1_SET_KEYBOARD_INTERACTIVITY: u32 = 4;
const ZWLR_LAYER_SURFACE_V1_ACK_CONFIGURE: u32 = 6;
const ZWLR_LAYER_SURFACE_V1_DESTROY: u32 = 7;

const ZWLR_LAYER_SHELL_V1_LAYER_BACKGROUND: u32 = 0;
const ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP: u32 = 1;
const ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM: u32 = 2;
const ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT: u32 = 4;
const ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT: u32 = 8;
const ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE: u32 = 0;

const EglGetPlatformDisplayFn = *const fn (u32, ?*anyopaque, ?[*]const i32) callconv(.c) ?*anyopaque;

extern fn wl_egl_window_create(*wl_surface, c_int, c_int) callconv(.c) ?*wl_egl_window;
extern fn wl_egl_window_destroy(*wl_egl_window) callconv(.c) void;
extern fn wl_egl_window_resize(*wl_egl_window, c_int, c_int, c_int, c_int) callconv(.c) void;

extern fn eglGetDisplay(?*anyopaque) callconv(.c) ?*anyopaque;
extern fn eglGetPlatformDisplay(u32, ?*anyopaque, ?[*]const i32) callconv(.c) ?*anyopaque;
extern fn eglGetProcAddress([*:0]const u8) callconv(.c) ?*const anyopaque;
extern fn eglInitialize(?*anyopaque, *i32, *i32) callconv(.c) u32;
extern fn eglTerminate(?*anyopaque) callconv(.c) u32;
extern fn eglChooseConfig(?*anyopaque, [*]const i32, [*]?*anyopaque, i32, *i32) callconv(.c) u32;
extern fn eglBindAPI(u32) callconv(.c) u32;
extern fn eglCreateContext(?*anyopaque, ?*anyopaque, ?*anyopaque, [*]const i32) callconv(.c) ?*anyopaque;
extern fn eglDestroyContext(?*anyopaque, ?*anyopaque) callconv(.c) u32;
extern fn eglCreateWindowSurface(?*anyopaque, ?*anyopaque, ?*anyopaque, ?[*]const i32) callconv(.c) ?*anyopaque;
extern fn eglDestroySurface(?*anyopaque, ?*anyopaque) callconv(.c) u32;
extern fn eglMakeCurrent(?*anyopaque, ?*anyopaque, ?*anyopaque, ?*anyopaque) callconv(.c) u32;
extern fn eglSwapBuffers(?*anyopaque, ?*anyopaque) callconv(.c) u32;
extern fn eglSwapInterval(?*anyopaque, i32) callconv(.c) u32;

extern fn glClearColor(f32, f32, f32, f32) callconv(.c) void;
extern fn glClear(u32) callconv(.c) void;
extern fn glViewport(c_int, c_int, c_int, c_int) callconv(.c) void;

const Runtime = struct {
    wayland_lib: std.DynLib,
    wayland: WaylandFns,

    fn load() !Runtime {
        if (builtin.os.tag != .linux) {
            return error.UnsupportedOS;
        }

        var wayland_lib = openAny(&.{ "libwayland-client.so.0", "libwayland-client.so" }) orelse {
            return error.WaylandLibNotFound;
        };
        errdefer wayland_lib.close();

        const runtime: Runtime = .{
            .wayland_lib = wayland_lib,
            .wayland = try WaylandFns.load(&wayland_lib),
        };

        return runtime;
    }

    fn deinit(self: *Runtime) void {
        self.wayland_lib.close();
    }
};

pub const Client = struct {
    runtime: Runtime,
    display: *wl_display,
    compositor: *wl_compositor,
    layer_shell: ?*zwlr_layer_shell_v1 = null,
    globals: Globals,
    egl_display: ?*anyopaque = null,
    egl_version: EglVersion = .{ .major = 0, .minor = 0 },

    pub fn connect() !Client {
        var runtime = try Runtime.load();
        errdefer runtime.deinit();

        const display = runtime.wayland.display_connect(null) orelse return error.DisplayConnectFailed;
        errdefer runtime.wayland.display_disconnect(display);

        const registry_proxy = runtime.wayland.proxy_marshal_flags(
            @ptrCast(display),
            WL_DISPLAY_GET_REGISTRY,
            runtime.wayland.registry_interface,
            runtime.wayland.proxy_get_version(@ptrCast(display)),
            0,
            @as(?*anyopaque, null),
        ) orelse return error.RegistryCreateFailed;
        defer runtime.wayland.proxy_destroy(registry_proxy);

        const registry: *wl_registry = @ptrCast(registry_proxy);

        var globals: Globals = .{};
        const listener_impl = @as([*]const ?*const fn () callconv(.c) void, @ptrCast(&registry_listener));
        if (runtime.wayland.proxy_add_listener(registry_proxy, listener_impl, &globals) != 0) {
            return error.RegistryListenerFailed;
        }

        if (runtime.wayland.display_roundtrip(display) < 0) {
            return error.RoundtripFailed;
        }

        const compositor_global = globals.compositor orelse return error.MissingCompositor;
        const compositor_proxy = bindRegistry(
            &runtime,
            registry,
            compositor_global,
            runtime.wayland.compositor_interface,
        ) orelse return error.BindCompositorFailed;
        const compositor: *wl_compositor = @ptrCast(compositor_proxy);
        var layer_shell: ?*zwlr_layer_shell_v1 = null;
        if (globals.layer_shell) |layer_shell_global| {
            const layer_shell_proxy = bindRegistry(
                &runtime,
                registry,
                layer_shell_global,
                &zwlr_layer_shell_v1_interface,
            ) orelse return error.BindLayerShellFailed;
            layer_shell = @ptrCast(layer_shell_proxy);
        }

        return .{
            .runtime = runtime,
            .display = display,
            .compositor = compositor,
            .layer_shell = layer_shell,
            .globals = globals,
        };
    }

    pub fn deinit(self: *Client) void {
        if (self.layer_shell) |layer_shell| {
            self.runtime.wayland.proxy_destroy(@ptrCast(layer_shell));
        }
        self.runtime.wayland.proxy_destroy(@ptrCast(self.compositor));
        if (self.egl_display) |egl_display| {
            _ = eglTerminate(egl_display);
        }
        self.runtime.wayland.display_disconnect(self.display);
        self.runtime.deinit();
        self.* = undefined;
    }

    pub fn ensureEglDisplay(self: *Client) !EglVersion {
        if (self.egl_display != null) {
            return self.egl_version;
        }

        const native_display = @as(?*anyopaque, @ptrCast(self.display));
        var egl_display: ?*anyopaque = null;

        egl_display = eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_KHR, native_display, null);
        if (egl_display == null) {
            if (eglGetProcAddress("eglGetPlatformDisplayEXT")) |symbol| {
                const get_platform_display_ext: EglGetPlatformDisplayFn = @ptrCast(symbol);
                egl_display = get_platform_display_ext(EGL_PLATFORM_WAYLAND_KHR, native_display, null);
            }
        }
        if (egl_display == null) {
            egl_display = eglGetDisplay(native_display);
        }
        if (egl_display == null) {
            return error.EglDisplayInitFailed;
        }

        var major: i32 = 0;
        var minor: i32 = 0;
        if (eglInitialize(egl_display.?, &major, &minor) == EGL_FALSE) {
            return error.EglDisplayInitFailed;
        }

        self.egl_display = egl_display;
        self.egl_version = .{
            .major = major,
            .minor = minor,
        };
        return self.egl_version;
    }

    pub fn createSurface(self: *Client) !Surface {
        const compositor_proxy: *wl_proxy = @ptrCast(self.compositor);
        const surface_proxy = self.runtime.wayland.proxy_marshal_flags(
            compositor_proxy,
            WL_COMPOSITOR_CREATE_SURFACE,
            self.runtime.wayland.surface_interface,
            self.runtime.wayland.proxy_get_version(compositor_proxy),
            0,
            @as(?*anyopaque, null),
        ) orelse return error.SurfaceCreateFailed;

        return .{
            .client = self,
            .handle = @ptrCast(surface_proxy),
        };
    }

    pub fn createEglWindow(
        self: *Client,
        surface: *const Surface,
        width: i32,
        height: i32,
    ) !EglWindow {
        _ = self;
        const window = wl_egl_window_create(
            surface.handle,
            @as(c_int, @intCast(width)),
            @as(c_int, @intCast(height)),
        ) orelse return error.WlEglWindowCreateFailed;

        return .{
            .handle = window,
        };
    }

    pub fn createBackgroundLayerSurface(
        self: *Client,
        surface: *const Surface,
        namespace: [*:0]const u8,
    ) !*zwlr_layer_surface_v1 {
        const layer_shell = self.layer_shell orelse return error.MissingLayerShell;
        const layer_shell_proxy: *wl_proxy = @ptrCast(layer_shell);
        const layer_surface_proxy = self.runtime.wayland.proxy_marshal_flags(
            layer_shell_proxy,
            ZWLR_LAYER_SHELL_V1_GET_LAYER_SURFACE,
            &zwlr_layer_surface_v1_interface,
            self.runtime.wayland.proxy_get_version(layer_shell_proxy),
            0,
            @as(?*anyopaque, null),
            surface.handle,
            @as(?*wl_output, null),
            ZWLR_LAYER_SHELL_V1_LAYER_BACKGROUND,
            namespace,
        ) orelse return error.LayerSurfaceCreateFailed;
        return @ptrCast(layer_surface_proxy);
    }

    pub fn commitSurface(self: *Client, surface: *const Surface) void {
        const surface_proxy: *wl_proxy = @ptrCast(surface.handle);
        _ = self.runtime.wayland.proxy_marshal_flags(
            surface_proxy,
            WL_SURFACE_COMMIT,
            null,
            self.runtime.wayland.proxy_get_version(surface_proxy),
            0,
        );
    }
};

pub const Surface = struct {
    client: *Client,
    handle: *wl_surface,

    pub fn deinit(self: *Surface) void {
        self.client.runtime.wayland.proxy_destroy(@ptrCast(self.handle));
        self.* = undefined;
    }
};

pub const EglWindow = struct {
    handle: *wl_egl_window,

    pub fn resize(self: *EglWindow, width: i32, height: i32) void {
        wl_egl_window_resize(
            self.handle,
            @as(c_int, @intCast(width)),
            @as(c_int, @intCast(height)),
            0,
            0,
        );
    }

    pub fn deinit(self: *EglWindow) void {
        wl_egl_window_destroy(self.handle);
        self.* = undefined;
    }
};

pub fn runGrayLayerShellAnimation() !void {
    var client = try Client.connect();
    defer client.deinit();

    const egl_version = try client.ensureEglDisplay();
    _ = egl_version;

    var background: LayerBackground = undefined;
    try background.init(&client);
    defer background.deinit();
    try background.run();
}

const LayerBackground = struct {
    client: *Client,
    surface: Surface,
    layer_surface: *zwlr_layer_surface_v1,
    state: LayerSurfaceState = undefined,
    egl_window: ?EglWindow = null,
    egl_context: ?*anyopaque = null,
    egl_surface: ?*anyopaque = null,
    width: i32 = 1280,
    height: i32 = 720,
    has_size: bool = false,

    fn init(self: *LayerBackground, client: *Client) !void {
        var surface = try client.createSurface();
        const layer_surface = client.createBackgroundLayerSurface(&surface, "waystream") catch |err| {
            surface.deinit();
            return err;
        };

        self.* = .{
            .client = client,
            .surface = surface,
            .layer_surface = layer_surface,
            .state = .{
                .client = client,
            },
        };
        errdefer self.deinit();

        const listener_impl = @as([*]const ?*const fn () callconv(.c) void, @ptrCast(&layer_surface_listener));
        if (client.runtime.wayland.proxy_add_listener(@ptrCast(layer_surface), listener_impl, &self.state) != 0) {
            return error.LayerSurfaceListenerFailed;
        }

        const layer_surface_proxy: *wl_proxy = @ptrCast(layer_surface);
        _ = client.runtime.wayland.proxy_marshal_flags(
            layer_surface_proxy,
            ZWLR_LAYER_SURFACE_V1_SET_SIZE,
            null,
            client.runtime.wayland.proxy_get_version(layer_surface_proxy),
            0,
            @as(u32, 0),
            @as(u32, 0),
        );
        _ = client.runtime.wayland.proxy_marshal_flags(
            layer_surface_proxy,
            ZWLR_LAYER_SURFACE_V1_SET_ANCHOR,
            null,
            client.runtime.wayland.proxy_get_version(layer_surface_proxy),
            0,
            ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP |
                ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM |
                ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT |
                ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT,
        );
        _ = client.runtime.wayland.proxy_marshal_flags(
            layer_surface_proxy,
            ZWLR_LAYER_SURFACE_V1_SET_EXCLUSIVE_ZONE,
            null,
            client.runtime.wayland.proxy_get_version(layer_surface_proxy),
            0,
            @as(i32, -1),
        );
        _ = client.runtime.wayland.proxy_marshal_flags(
            layer_surface_proxy,
            ZWLR_LAYER_SURFACE_V1_SET_KEYBOARD_INTERACTIVITY,
            null,
            client.runtime.wayland.proxy_get_version(layer_surface_proxy),
            0,
            ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE,
        );
        client.commitSurface(&surface);

        while (!self.state.configured and !self.state.closed) {
            if (client.runtime.wayland.display_dispatch(client.display) < 0) {
                return error.LayerSurfaceConfigureFailed;
            }
        }
        if (self.state.closed) {
            return error.LayerSurfaceClosed;
        }

        if (self.state.width > 0 and self.state.height > 0) {
            self.width = self.state.width;
            self.height = self.state.height;
            self.has_size = true;
            self.state.needs_resize = false;
        }

        self.egl_window = try client.createEglWindow(&self.surface, self.width, self.height);
        try self.initEglRenderer();
    }

    fn initEglRenderer(self: *LayerBackground) !void {
        const egl_display = self.client.egl_display orelse return error.EglDisplayInitFailed;
        const egl_window = self.egl_window orelse return error.WlEglWindowCreateFailed;

        const config_attribs = [_]i32{
            EGL_SURFACE_TYPE,    EGL_WINDOW_BIT,
            EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
            EGL_RED_SIZE,        8,
            EGL_GREEN_SIZE,      8,
            EGL_BLUE_SIZE,       8,
            EGL_ALPHA_SIZE,      8,
            EGL_NONE,
        };
        var config: ?*anyopaque = null;
        var num_config: i32 = 0;
        if (eglChooseConfig(
            egl_display,
            &config_attribs,
            @ptrCast(&config),
            1,
            &num_config,
        ) == EGL_FALSE or config == null or num_config < 1) {
            return error.EglConfigFailed;
        }

        if (eglBindAPI(EGL_OPENGL_ES_API) == EGL_FALSE) {
            return error.EglContextCreateFailed;
        }
        const context_attribs = [_]i32{
            EGL_CONTEXT_CLIENT_VERSION, 2,
            EGL_NONE,
        };
        const egl_context = eglCreateContext(
            egl_display,
            config,
            null,
            &context_attribs,
        ) orelse return error.EglContextCreateFailed;
        errdefer _ = eglDestroyContext(egl_display, egl_context);

        const egl_surface = eglCreateWindowSurface(
            egl_display,
            config,
            @ptrCast(egl_window.handle),
            null,
        ) orelse return error.EglWindowSurfaceCreateFailed;
        errdefer _ = eglDestroySurface(egl_display, egl_surface);

        if (eglMakeCurrent(egl_display, egl_surface, egl_surface, egl_context) == EGL_FALSE) {
            return error.EglMakeCurrentFailed;
        }
        _ = eglSwapInterval(egl_display, 1);
        glViewport(0, 0, @as(c_int, @intCast(self.width)), @as(c_int, @intCast(self.height)));

        self.egl_context = egl_context;
        self.egl_surface = egl_surface;
    }

    fn run(self: *LayerBackground) !void {
        const egl_display = self.client.egl_display orelse return error.EglDisplayInitFailed;
        const egl_surface = self.egl_surface orelse return error.EglWindowSurfaceCreateFailed;
        const frame_time_ns: u64 = std.time.ns_per_s / 60;
        const start_ns = std.time.nanoTimestamp();

        while (!self.state.closed) {
            if (self.client.runtime.wayland.display_dispatch_pending(self.client.display) < 0) {
                return error.DispatchFailed;
            }

            if (self.state.needs_resize and self.state.width > 0 and self.state.height > 0) {
                self.width = self.state.width;
                self.height = self.state.height;
                if (self.egl_window) |*egl_window_state| {
                    egl_window_state.resize(self.width, self.height);
                }
                glViewport(
                    0,
                    0,
                    @as(c_int, @intCast(self.width)),
                    @as(c_int, @intCast(self.height)),
                );
                self.state.needs_resize = false;
            }

            const now_ns = std.time.nanoTimestamp();
            const elapsed_s = @as(f64, @floatFromInt(now_ns - start_ns)) / @as(f64, @floatFromInt(std.time.ns_per_s));
            const wave = 0.5 + 0.5 * std.math.sin(elapsed_s * 0.8);
            const gray = 0.15 + 0.70 * wave;
            glClearColor(
                @as(f32, @floatCast(gray)),
                @as(f32, @floatCast(gray)),
                @as(f32, @floatCast(gray)),
                1.0,
            );
            glClear(GL_COLOR_BUFFER_BIT);
            if (eglSwapBuffers(egl_display, egl_surface) == EGL_FALSE) {
                return error.EglSwapFailed;
            }

            _ = self.client.runtime.wayland.display_flush(self.client.display);
            std.Thread.sleep(frame_time_ns);
        }
    }

    fn deinit(self: *LayerBackground) void {
        const egl_display = self.client.egl_display;
        if (egl_display) |display| {
            if (self.egl_context != null or self.egl_surface != null) {
                _ = eglMakeCurrent(display, null, null, null);
            }
            if (self.egl_surface) |surface| {
                _ = eglDestroySurface(display, surface);
            }
            if (self.egl_context) |context| {
                _ = eglDestroyContext(display, context);
            }
        }
        if (self.egl_window) |*egl_window| {
            egl_window.deinit();
        }
        _ = self.client.runtime.wayland.proxy_marshal_flags(
            @ptrCast(self.layer_surface),
            ZWLR_LAYER_SURFACE_V1_DESTROY,
            null,
            self.client.runtime.wayland.proxy_get_version(@ptrCast(self.layer_surface)),
            WL_MARSHAL_FLAG_DESTROY,
        );
        self.surface.deinit();
        self.* = undefined;
    }
};

const LayerSurfaceState = struct {
    client: *Client,
    configured: bool = false,
    closed: bool = false,
    width: i32 = 0,
    height: i32 = 0,
    needs_resize: bool = false,
};

fn openAny(candidates: []const []const u8) ?std.DynLib {
    for (candidates) |candidate| {
        if (std.DynLib.open(candidate)) |lib| {
            return lib;
        } else |_| {}
    }
    return null;
}

fn bindRegistry(
    runtime: *const Runtime,
    registry: *wl_registry,
    global: Global,
    interface: *const wl_interface,
) ?*wl_proxy {
    const interface_version = if (interface.version > 0)
        @as(u32, @intCast(interface.version))
    else
        @as(u32, 1);
    const version = @min(interface_version, global.version);
    return runtime.wayland.proxy_marshal_flags(
        @ptrCast(registry),
        WL_REGISTRY_BIND,
        interface,
        version,
        0,
        global.name,
        interface.name,
        version,
        @as(?*anyopaque, null),
    );
}

fn onGlobal(
    data: ?*anyopaque,
    registry: *wl_registry,
    name: u32,
    interface: [*:0]const u8,
    version: u32,
) callconv(.c) void {
    _ = registry;
    const ptr = data orelse return;
    const globals: *Globals = @ptrCast(@alignCast(ptr));
    const iface = std.mem.span(interface);

    if (std.mem.eql(u8, iface, "wl_compositor")) {
        globals.compositor = .{ .name = name, .version = version };
        return;
    }
    if (std.mem.eql(u8, iface, "wl_shm")) {
        globals.shm = .{ .name = name, .version = version };
        return;
    }
    if (std.mem.eql(u8, iface, "xdg_wm_base")) {
        globals.xdg_wm_base = .{ .name = name, .version = version };
        return;
    }
    if (std.mem.eql(u8, iface, "zwlr_layer_shell_v1")) {
        globals.layer_shell = .{ .name = name, .version = version };
    }
}

fn onGlobalRemove(
    data: ?*anyopaque,
    registry: *wl_registry,
    name: u32,
) callconv(.c) void {
    _ = data;
    _ = registry;
    _ = name;
}

const registry_listener: wl_registry_listener = .{
    .global = onGlobal,
    .global_remove = onGlobalRemove,
};

const zwlr_layer_surface_v1_listener = extern struct {
    configure: ?*const fn (
        data: ?*anyopaque,
        layer_surface: *zwlr_layer_surface_v1,
        serial: u32,
        width: u32,
        height: u32,
    ) callconv(.c) void,
    closed: ?*const fn (
        data: ?*anyopaque,
        layer_surface: *zwlr_layer_surface_v1,
    ) callconv(.c) void,
};

fn onLayerSurfaceConfigure(
    data: ?*anyopaque,
    layer_surface: *zwlr_layer_surface_v1,
    serial: u32,
    width: u32,
    height: u32,
) callconv(.c) void {
    const ptr = data orelse return;
    const state: *LayerSurfaceState = @ptrCast(@alignCast(ptr));
    const layer_surface_proxy: *wl_proxy = @ptrCast(layer_surface);
    _ = state.client.runtime.wayland.proxy_marshal_flags(
        layer_surface_proxy,
        ZWLR_LAYER_SURFACE_V1_ACK_CONFIGURE,
        null,
        state.client.runtime.wayland.proxy_get_version(layer_surface_proxy),
        0,
        serial,
    );

    if (width > 0 and height > 0) {
        state.width = @as(i32, @intCast(width));
        state.height = @as(i32, @intCast(height));
        state.needs_resize = true;
    }
    state.configured = true;
}

fn onLayerSurfaceClosed(
    data: ?*anyopaque,
    layer_surface: *zwlr_layer_surface_v1,
) callconv(.c) void {
    _ = layer_surface;
    const ptr = data orelse return;
    const state: *LayerSurfaceState = @ptrCast(@alignCast(ptr));
    state.closed = true;
}

const layer_surface_listener: zwlr_layer_surface_v1_listener = .{
    .configure = onLayerSurfaceConfigure,
    .closed = onLayerSurfaceClosed,
};

const zwlr_layer_shell_v1_methods = [_]wl_message{
    .{ .name = "get_layer_surface", .signature = "no?ous", .types = null },
    .{ .name = "destroy", .signature = "", .types = null },
};

const zwlr_layer_shell_v1_interface = wl_interface{
    .name = "zwlr_layer_shell_v1",
    .version = 5,
    .method_count = @as(c_int, zwlr_layer_shell_v1_methods.len),
    .methods = &zwlr_layer_shell_v1_methods,
    .event_count = 0,
    .events = null,
};

const zwlr_layer_surface_v1_methods = [_]wl_message{
    .{ .name = "set_size", .signature = "uu", .types = null },
    .{ .name = "set_anchor", .signature = "u", .types = null },
    .{ .name = "set_exclusive_zone", .signature = "i", .types = null },
    .{ .name = "set_margin", .signature = "iiii", .types = null },
    .{ .name = "set_keyboard_interactivity", .signature = "u", .types = null },
    .{ .name = "get_popup", .signature = "o", .types = null },
    .{ .name = "ack_configure", .signature = "u", .types = null },
    .{ .name = "destroy", .signature = "", .types = null },
    .{ .name = "set_layer", .signature = "2u", .types = null },
    .{ .name = "set_exclusive_edge", .signature = "4u", .types = null },
};

const zwlr_layer_surface_v1_events = [_]wl_message{
    .{ .name = "configure", .signature = "uuu", .types = null },
    .{ .name = "closed", .signature = "", .types = null },
};

const zwlr_layer_surface_v1_interface = wl_interface{
    .name = "zwlr_layer_surface_v1",
    .version = 5,
    .method_count = @as(c_int, zwlr_layer_surface_v1_methods.len),
    .methods = &zwlr_layer_surface_v1_methods,
    .event_count = @as(c_int, zwlr_layer_surface_v1_events.len),
    .events = &zwlr_layer_surface_v1_events,
};

const WaylandFns = struct {
    display_connect: *const fn (?[*:0]const u8) callconv(.c) ?*wl_display,
    display_disconnect: *const fn (*wl_display) callconv(.c) void,
    display_dispatch: *const fn (*wl_display) callconv(.c) c_int,
    display_dispatch_pending: *const fn (*wl_display) callconv(.c) c_int,
    display_flush: *const fn (*wl_display) callconv(.c) c_int,
    display_roundtrip: *const fn (*wl_display) callconv(.c) c_int,
    proxy_add_listener: *const fn (*wl_proxy, [*]const ?*const fn () callconv(.c) void, ?*anyopaque) callconv(.c) c_int,
    proxy_destroy: *const fn (*wl_proxy) callconv(.c) void,
    proxy_get_version: *const fn (*wl_proxy) callconv(.c) u32,
    proxy_marshal_flags: *const fn (*wl_proxy, u32, ?*const wl_interface, u32, u32, ...) callconv(.c) ?*wl_proxy,
    registry_interface: *const wl_interface,
    compositor_interface: *const wl_interface,
    surface_interface: *const wl_interface,

    fn load(lib: *std.DynLib) !WaylandFns {
        return .{
            .display_connect = try loadRequired(
                *const fn (?[*:0]const u8) callconv(.c) ?*wl_display,
                lib,
                "wl_display_connect",
            ),
            .display_disconnect = try loadRequired(
                *const fn (*wl_display) callconv(.c) void,
                lib,
                "wl_display_disconnect",
            ),
            .display_dispatch = try loadRequired(
                *const fn (*wl_display) callconv(.c) c_int,
                lib,
                "wl_display_dispatch",
            ),
            .display_dispatch_pending = try loadRequired(
                *const fn (*wl_display) callconv(.c) c_int,
                lib,
                "wl_display_dispatch_pending",
            ),
            .display_flush = try loadRequired(
                *const fn (*wl_display) callconv(.c) c_int,
                lib,
                "wl_display_flush",
            ),
            .display_roundtrip = try loadRequired(
                *const fn (*wl_display) callconv(.c) c_int,
                lib,
                "wl_display_roundtrip",
            ),
            .proxy_add_listener = try loadRequired(
                *const fn (*wl_proxy, [*]const ?*const fn () callconv(.c) void, ?*anyopaque) callconv(.c) c_int,
                lib,
                "wl_proxy_add_listener",
            ),
            .proxy_destroy = try loadRequired(
                *const fn (*wl_proxy) callconv(.c) void,
                lib,
                "wl_proxy_destroy",
            ),
            .proxy_get_version = try loadRequired(
                *const fn (*wl_proxy) callconv(.c) u32,
                lib,
                "wl_proxy_get_version",
            ),
            .proxy_marshal_flags = try loadRequired(
                *const fn (*wl_proxy, u32, ?*const wl_interface, u32, u32, ...) callconv(.c) ?*wl_proxy,
                lib,
                "wl_proxy_marshal_flags",
            ),
            .registry_interface = try loadRequired(
                *const wl_interface,
                lib,
                "wl_registry_interface",
            ),
            .compositor_interface = try loadRequired(
                *const wl_interface,
                lib,
                "wl_compositor_interface",
            ),
            .surface_interface = try loadRequired(
                *const wl_interface,
                lib,
                "wl_surface_interface",
            ),
        };
    }
};

fn loadRequired(comptime T: type, lib: *std.DynLib, name: [:0]const u8) !T {
    return lib.lookup(T, name) orelse {
        log.err("required symbol missing: {s}", .{name});
        return error.WaylandSymbolMissing;
    };
}
