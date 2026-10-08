//! A fordító verziójának átadása a futó programnak (az admin panel Rendszer nézetéhez).

fn main() {
    let version = std::process::Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string()))
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.split_whitespace().nth(1).unwrap_or("").to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=SCRABBLE_RUSTC_VERSION={version}");
    println!("cargo:rerun-if-changed=build.rs");
}
