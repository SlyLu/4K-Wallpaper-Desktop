// Release builds must not attach a console whose lifetime can terminate the desktop application.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = wallpaper_desktop_lib::run() {
        eprintln!("4K Wallpaper Desktop failed: {error}");
        std::process::exit(1);
    }
}
