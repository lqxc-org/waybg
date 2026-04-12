const c = @cImport({
    // Newest syncobj v1 not support on old platform
    //@cInclude("linux-drm-syncobj-v1-client-protocol.h");
    //@cInclude("drm_fourcc.h");
});
