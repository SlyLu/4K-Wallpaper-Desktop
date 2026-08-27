use std::{env, path::PathBuf, process::Command};

/// Ensures the GNU Windows bundle has the Microsoft loader before Tauri reads
/// the resource list. The script is Windows-only and does no work for macOS.
fn prepare_windows_webview2_loader() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| "src-tauri".into()));
    let script = manifest_dir.join("../scripts/prepare-webview2-loader.ps1");
    let destination = manifest_dir.join("resources/windows/x64/WebView2Loader.dll");

    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-changed={}", destination.display());

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Destination")
        .arg(&destination)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to start WebView2 SDK preparation script {}: {error}",
                script.display()
            )
        });

    assert!(
        status.success(),
        "WebView2 SDK preparation failed with exit code {:?}",
        status.code()
    );
}

fn main() {
    prepare_windows_webview2_loader();
    tauri_build::build()
}
