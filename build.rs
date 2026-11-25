//! Puts the icon and version details into `undavi.exe`, for the taskbar, Explorer and
//! the volume mixer. Does nothing on other platforms.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=assets/icons/windows/undavi.ico");
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/icons/windows/undavi.ico")
            .set("ProductName", "Undavi")
            .set("FileDescription", "Undavi")
            .set("LegalCopyright", "\u{00A9} 2026 Karson Califf");
        if let Err(e) = resource.compile() {
            println!("cargo::warning=undavi.exe gets no icon: {e}");
        }
    }
}
