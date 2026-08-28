# ADR 0009: V2 Core scope and release gates

- Status: Accepted
- Date: 2026-08-28

## Context

V2 adds a large Core roadmap and three Experimental capabilities. Treating every experiment as a release blocker would make the stable static-wallpaper path depend on platform-specific desktop hosts, large local models, and signing infrastructure that is not currently available.

## Decision

- V2 Core is limited to Gallery 2.0, collections, rotation 2.0, the provider center, static spanning, themes, migration, diagnostics, and packaging.
- Dynamic/video wallpaper, local semantic search, and automatic updates remain independently gated Experimental features. A failed experiment cannot degrade static wallpapers.
- Local AI models may only be downloaded after explicit consent. No model is bundled, and the initial download ceiling is 1.5 GiB until a benchmark ADR proves a larger model is necessary.
- Static spanning supports arbitrary rectangular monitor positions represented by the operating-system virtual desktop. Rotation is handled as part of the monitor rectangle; irregular non-rectangular physical bezels are outside V2 Core.
- LocalProvider never permanently deletes a user source file. Core operations may remove an index, cache, or application-generated derivative only.
- Windows and macOS may publish separate artifacts, but a shared Core feature is not marked cross-platform complete until both target systems have been tested.
- GitHub Releases remains the distribution channel. Automatic update stays Experimental until Windows signing and Apple Developer ID/notarization are available.

## Consequences

Core development can proceed in phase order without importing an unstable dynamic host or AI runtime. Experimental services must remain optional modules with static fallbacks. Platform verification reports must distinguish Windows results from unverified macOS behavior.
