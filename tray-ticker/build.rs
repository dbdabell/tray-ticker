//! Generates `assets/app.ico` if missing, then embeds resources via `embed-resource`.

fn main() {
    let assets = std::path::Path::new("assets");
    let _ = std::fs::create_dir_all(assets);

    let ico_path = assets.join("app.ico");
    if !ico_path.exists() {
        let mut img = image::ImageBuffer::<image::Rgba<u8>, _>::new(32, 32);
        for p in img.pixels_mut() {
            *p = image::Rgba([0x44, 0xaa, 0xff, 255]);
        }
        let dyn_img = image::DynamicImage::ImageRgba8(img);
        let mut buf = std::io::Cursor::new(Vec::new());
        dyn_img
            .write_to(&mut buf, image::ImageFormat::Ico)
            .expect("write generated ico");
        std::fs::write(&ico_path, buf.into_inner()).expect("save app.ico");
    }

    let default_ico = assets.join("default.ico");
    if !default_ico.exists() {
        std::fs::copy(&ico_path, &default_ico).expect("copy default.ico");
    }

    #[cfg(windows)]
    {
        embed_resource::compile("assets/app.rc", embed_resource::NONE);
        println!("cargo:rerun-if-changed=assets/app.rc");
        println!("cargo:rerun-if-changed=assets/app.manifest");
        println!("cargo:rerun-if-changed=assets/app.ico");
    }
}
