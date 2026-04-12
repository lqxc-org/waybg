const c = @cImport({
    @cInclude("mpv/client.h");
});

pub const MpvError = error{
    // fmt
    CtxCreateFailed,
};

pub const MpvInterface = struct {
    ctx: *c.mpv_handle,

    pub fn init() MpvError!@This() {
        if (c.mpv_create()) |ctx| {
            return .{ .ctx = ctx };
        } else {
            return error.CtxCreateFailed;
        }
    }
};
