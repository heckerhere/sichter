fn main() {
    // Ziel-Triple für die Suche nach Sidecars im Entwicklungsmodus.
    println!("cargo:rustc-env=SICHTER_TARGET={}", std::env::var("TARGET").unwrap_or_default());
    tauri_build::build()
}
