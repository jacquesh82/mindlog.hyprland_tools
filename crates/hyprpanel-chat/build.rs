fn main() {
    // Les transformations Slint peuvent être rasterisées sur CPU. Préparer les
    // textures à la taille utile (100 px, avec marge HiDPI), une fois au build.
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for (source, target) in [
        ("assets/lyra-avatar.png", "avatar-open.rgba"),
        ("assets/lyra-avatar-blink.png", "avatar-blink.rgba"),
    ] {
        println!("cargo:rerun-if-changed={source}");
        let pixels = image::open(source)
            .expect("avatar PNG")
            .resize_exact(256, 256, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        std::fs::write(out.join(target), pixels.as_raw()).expect("texture avatar");
    }
    slint_build::compile("ui/chat.slint").expect("compilation Slint");
}
