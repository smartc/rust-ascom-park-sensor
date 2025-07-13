fn main() {
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"));
}