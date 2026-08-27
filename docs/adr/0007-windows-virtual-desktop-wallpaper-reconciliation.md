# ADR 0007: Reconcile wallpaper assignments across Windows virtual desktops

## Status

Accepted

## Context

Windows 11 can retain a different wallpaper for each virtual desktop. `IDesktopWallpaper::SetWallpaper` updates the active desktop, but switching to another virtual desktop can restore that desktop's older per-monitor assignments. The public Windows wallpaper interface does not expose a stable virtual-desktop change notification suitable for the existing adapter contract.

## Decision

Extend the platform-neutral wallpaper service with an optional reconciliation operation. The Windows adapter remembers only successful per-monitor assignments and compares them with the active desktop once per second. A mismatch is reapplied through the same `IDesktopWallpaper` API. The macOS adapter uses the default no-op implementation.

## Consequences

- Virtual-desktop transitions restore the selected wallpapers within approximately one second.
- Multi-monitor assignments remain independent because the desired state is keyed by the native monitor identifier.
- Windows-specific COM and path handling remain isolated in `platform/windows`.
- While the application is running, its most recently applied wallpaper remains authoritative over external wallpaper changes.
