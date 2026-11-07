use std::{env, fs::File, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../../assets/bridge-app-icon.png");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let source = "../../assets/bridge-app-icon.png";
    let image = image::open(source).expect("failed to load Bridge app icon").to_rgba8();
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let resized = image::imageops::resize(&image, size, size, image::imageops::FilterType::Lanczos3);
        let icon_image = ico::IconImage::from_rgba_data(size, size, resized.into_raw());
        icon_dir.add_entry(ico::IconDirEntry::encode(&icon_image).expect("failed to encode ICO entry"));
    }

    let icon_path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing")).join("bridge.ico");
    icon_dir.write(File::create(&icon_path).expect("failed to create ICO")).expect("failed to write ICO");
    winres::WindowsResource::new()
        .set_icon(icon_path.to_str().expect("ICO path is not UTF-8"))
        .compile()
        .expect("failed to embed Bridge Windows icon");
}
