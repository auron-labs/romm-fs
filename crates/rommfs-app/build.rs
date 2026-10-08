#[path = "src/icon.rs"]
mod icon;
fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var_os("CARGO_FEATURE_UI").is_none()
    {
        return;
    }
    let path =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"))
            .join("rommfs.ico");
    std::fs::write(&path, icon::ico(32)).expect("write application icon");
    winresource::WindowsResource::new()
        .set_icon(path.to_str().expect("Unicode resource path"))
        .set("FileDescription", "RomMFS")
        .set("ProductName", "RomMFS")
        .compile()
        .expect("compile Windows executable icon resource");
}
