# ADR-0001: Windows Rust toolchain in the current validation environment

- Status: Accepted for local development validation
- Date: 2026-08-20

## Context

The Windows 11 validation host does not provide Visual Studio Build Tools and the current process cannot install machine-wide packages. Tauri 2 requires a native Windows linker.

## Decision

Use the official stable `x86_64-pc-windows-gnu` Rust toolchain for this host. Current Rust distributions include a self-contained MinGW linker, so this preserves Tauri 2, Rust, WebView2, SQLite, and the Windows native API architecture without introducing another application framework.

The `pnpm tauri` script exposes Rustup and, when present, a portable MSYS2 MinGW64 toolchain under the per-user `CodexToolchains` directory. The complete MinGW64 toolchain compiles bundled C dependencies such as SQLite and matches Rust's GNU ABI. The script does not hardcode a user profile or Rust version.

## Consequences

Windows development and runtime behavior must be validated with the GNU build output in this environment. Release CI may additionally build the conventional MSVC target once Visual Studio Build Tools are available. The source does not depend on toolchain-specific business behavior.
