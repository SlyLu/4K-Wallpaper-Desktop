# ADR 0006: Package WebView2Loader for the Windows GNU target

## Status

Accepted

## Context

The current Windows build environment uses `x86_64-pc-windows-gnu`. Its WebView2 bindings dynamically load `WebView2Loader.dll`. The Tauri 2.11.4 MSI bundle included the loader automatically, but the generated NSIS bundle omitted it when building without an explicit target triple. The installed executable consequently failed before application startup.

## Decision

Keep the required Tauri/Rust architecture and add a Windows-only bundle overlay. It packages the generated x64 loader as a resource, then an NSIS post-install hook copies it beside `wallpaper-desktop.exe`. Installation aborts if the required file is still absent. The uninstall hook removes the copied file.

## Consequences

- NSIS installations work with the established GNU Rust toolchain.
- macOS bundles remain unaffected by the Windows-only configuration.
- The NSIS package temporarily contains a second loader copy under `resources`; the size impact is small and favors a deterministic clean build.
- If the project later standardizes on MSVC, this compatibility rule can be removed after installer-level verification.
