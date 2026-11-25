//! Windows: gives the exe its icon and version details (shown in Explorer,
//! the taskbar and Task Manager). Nothing to do on other platforms.

fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icons/icon.ico");
        res.set("ProductName", "Franconvert");
        res.set("FileDescription", "Franconvert");
        res.set("CompanyName", "franconvert");
        res.set("LegalCopyright", "MIT License");
        res.compile().expect("embed Windows resources");
    }
}
