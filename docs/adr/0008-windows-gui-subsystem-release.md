# ADR 0008: Build Windows releases as GUI applications

## Status

Accepted

## Context

The Rust binary entry point did not select the Windows GUI subsystem. GNU Rust therefore emitted a Console UI executable, which opened an accompanying command window. Closing that window terminated the wallpaper process.

## Decision

Apply Rust's `windows_subsystem = "windows"` crate attribute to non-debug builds. Debug builds retain console output for local development, while release and installer builds run as normal desktop applications.

## Consequences

- Installed releases no longer open or depend on a command window.
- Closing a terminal used to launch the installed executable does not terminate it.
- Fatal startup diagnostics in release builds are written through the application's existing file logging when initialization reaches the logging stage.
