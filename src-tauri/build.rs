fn main() {
    // Release builds ask Windows for administrator rights at launch: creating
    // the TUN adapter needs them, and failing later with a vague error is
    // worse than one UAC prompt. Debug builds stay unelevated so `tauri dev`
    // and `cargo test` run from a normal terminal.
    let mut windows = tauri_build::WindowsAttributes::new();
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        windows = windows.app_manifest(include_str!("windows-app-manifest.xml"));
    }
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("tauri build failed");
}
