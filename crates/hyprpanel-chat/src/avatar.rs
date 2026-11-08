//! Textures partagées, pré-réduites au build : aucun décodage/redimensionnement
//! des PNG 1254² à chaque pose de l'avatar.
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};

pub fn install(app: &crate::ChatWindow) {
    let frames = app.global::<crate::AvatarFrames>();
    frames.set_open(Image::from_rgba8(
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            include_bytes!(concat!(env!("OUT_DIR"), "/avatar-open.rgba")),
            256,
            256,
        ),
    ));
    frames.set_blink(Image::from_rgba8(
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            include_bytes!(concat!(env!("OUT_DIR"), "/avatar-blink.rgba")),
            256,
            256,
        ),
    ));
}
