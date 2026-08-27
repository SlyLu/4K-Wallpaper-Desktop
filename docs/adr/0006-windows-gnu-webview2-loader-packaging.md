# ADR 0006: Package WebView2Loader for the Windows GNU target

## Status

Accepted

## Context

The current Windows build environment uses `x86_64-pc-windows-gnu`. Its WebView2 bindings dynamically load `WebView2Loader.dll`. The Tauri 2.11.4 MSI bundle included the loader automatically, but the generated NSIS bundle omitted it when building without an explicit target triple. The installed executable consequently failed before application startup.

## Decision

Keep the required Tauri/Rust architecture and add a Windows-only bundle overlay. Before Tauri reads the bundle resource list, `build.rs` invokes `scripts/prepare-webview2-loader.ps1`. If the loader is absent or invalid, the script downloads the pinned official `Microsoft.Web.WebView2` SDK 1.0.3650.58 package from NuGet, validates both the package and extracted x64 loader with SHA-256, and writes it to `src-tauri/resources/windows/x64/`. The generated DLL remains ignored by Git.

The bundle packages that validated loader as a resource, then an NSIS post-install hook copies it beside `wallpaper-desktop.exe`. Installation aborts if the required file is still absent. The uninstall hook removes the copied file.

## Consequences

- A clean Windows checkout can run `cargo check`, `cargo test`, or `pnpm tauri build` without a pre-existing `target` directory.
- The first Windows Rust build requires access to `api.nuget.org`; subsequent builds reuse the checksummed local loader.
- NSIS installations work with the established GNU Rust toolchain.
- macOS bundles remain unaffected by the Windows-only configuration.
- The NSIS package temporarily contains a second loader copy under `resources`; the size impact is small and favors a deterministic clean build.
- The SDK version intentionally follows the loader bundled by `webview2-com-sys` 0.38.2. Updating that Rust dependency requires reviewing the pinned SDK version and checksums together.
- If the project later standardizes on MSVC, this compatibility rule can be removed after installer-level verification.
