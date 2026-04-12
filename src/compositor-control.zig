//! As we current only supporting Wayland bg, but, in the future, we'd like to providing the ability
//! to render application/video/images/web to the background of the compositor like Windows/MacOS/Android/IOS/VR

pub fn CompositorControl(comptime CompositorControlImpl: type) type {
    if (!@hasDecl(CompositorControlImpl, "connect") || !@hasDecl(CompositorControlImpl, "disconnect")) {
        @compileError("The Compositor Control Impl lacks of connect and disconnect");
    }

    return struct {
        // fmt
        inner: CompositorControlImpl,

        pub fn connect() @This() {}
    };
}
