# ADR-0002: Deterministic Windows set-all behavior

- Status: Accepted
- Date: 2026-08-20

## Context

Microsoft documents that `IDesktopWallpaper::SetWallpaper` with a null monitor ID sets the image on all monitors. On the current Windows 11 dual-monitor host, the call returned success but an immediate per-monitor read showed that at least one independently configured display retained its prior image.

## Decision

The Windows adapter enumerates active monitor device IDs and applies the same image to each ID. This remains entirely inside the platform adapter and uses the same native API. Detached monitor records are excluded using their empty bounds.

## Consequences

Set-all performs one native operation per active display and can report an error if any individual display rejects the image. The behavior is deterministic for unified and previously independent multi-monitor configurations.
