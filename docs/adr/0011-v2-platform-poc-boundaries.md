# ADR 0011: V2 platform PoC boundaries

- Status: Accepted
- Date: 2026-08-28

## Context

Static spanning, video wallpaper, and semantic search have different platform and resource risks. Phase 0 must prevent experimental choices from leaking into the stable platform adapters.

## Decision

### Static spanning

- The shared image processor builds a virtual canvas from signed monitor coordinates and produces one immutable slice per monitor.
- Windows may use `IDesktopWallpaper` span support when applying one precomposed canvas, but per-monitor slices remain the portable fallback and the diagnostic path.
- macOS continues to apply a local image URL to each `NSScreen`; shared span rendering does not depend on a macOS-only business rule.

### Dynamic wallpaper

- Neither target platform's supported static wallpaper API is treated as a video host.
- Undocumented Windows WorkerW/Progman embedding is not accepted for V2 Core. A future Experimental adapter must prove cleanup, Explorer restart recovery, sleep/fullscreen pause, and static fallback.
- macOS requires a separate native host PoC and must not be reported as verified from Windows results.

### Local semantic search

- No inference runtime or model is added to Core during Phase 0.
- A later Experimental benchmark must measure cold start, one-image indexing latency, batch throughput, peak memory, index size, and CPU-only query latency on both target machines.
- The acceptance gate is opt-in installation, bounded storage, removable model/index data, and no effect on metadata search when unavailable.

## Consequences

Phase 0 establishes safe architecture and negative gates without shipping a fake dynamic or AI implementation. Static spanning can be implemented and tested independently; Experimental features remain disabled until their platform-specific benchmarks exist.

## References

- https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-idesktopwallpaper
- https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/ne-shobjidl_core-desktop_wallpaper_position
- https://developer.apple.com/documentation/appkit/nsworkspace/setdesktopimageurl(_:for:options:)
