
fn main() {
    // Generate Build Timestamp
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"));

    // App Icon Generation
    #[cfg(windows)]
    use std::path::Path;
    
    // Only embed icon on Windows
    if Path::new("assets/icon.ico").exists() {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set_version_info(winres::VersionInfo::PRODUCTVERSION, 0x0003000100000000);
        res.set_version_info(winres::VersionInfo::FILEVERSION, 0x0003000100000000);
        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to embed icon: {}", e);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    // Generate Build Timestamp
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"));
}