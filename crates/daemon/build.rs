//! On Windows, embeds the app icon and name into mirza.exe, so Explorer, the
//! taskbar and the Start menu show Mirza's icon.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/mirza.ico");
    let target = |key| std::env::var(key).unwrap_or_default();
    if target("CARGO_CFG_TARGET_OS") == "windows" && target("CARGO_CFG_TARGET_ENV") == "msvc" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/mirza.ico")
            .set("FileDescription", "Mirza")
            .set("ProductName", "Mirza")
            .set("OriginalFilename", "mirza.exe");
        res.compile().expect("embedding the icon in mirza.exe");
    }
}
