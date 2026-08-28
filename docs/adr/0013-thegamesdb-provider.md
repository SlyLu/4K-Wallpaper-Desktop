# ADR 0013: Use TheGamesDB as the first game-art Provider

## Status

Accepted for the V2 local-first Provider center.

## Context

The application needs game-focused results that match a searched title and expose real image dimensions before an original is downloaded. Scraping wallpaper websites is not acceptable, and a desktop binary cannot safely contain a shared private API credential.

## Decision

Add TheGamesDB through its documented REST API as an optional, disabled-by-default Provider. Users supply their own API Key in local application settings. Search first resolves game IDs by title, then requests only `fanart` and `screenshot` metadata for those games. Results must be SFW, landscape, use a supported raster format, satisfy the active resolution floor, and retain a TheGamesDB source-page link.

The adapter participates in the existing aggregated search, health isolation, fair interleaving, thumbnail perceptual hashing, and download-time SHA-256 validation. Generic latest/random refreshes skip this source because its API is title-oriented and monthly allowances should not be consumed by unrelated requests.

## Consequences

- No server or hard-coded API key is introduced.
- Game searches gain a source with explicit image type and resolution metadata.
- The provider remains unavailable until the user saves a key and enables it.
- Community artwork does not expose a reliable per-image license through the API. The application preserves attribution and leaves license fields unknown instead of inventing permission.
- API availability, allowance, and content quality remain external risks; failures are isolated from other Providers and offline rotation.

## Alternatives considered

- RAWG offers strong game metadata but its screenshot response does not consistently provide dimensions needed by the current metadata-first 4K filter.
- IGDB requires confidential Twitch application credentials and is unsuitable for direct distribution in a serverless desktop client.
- SteamGridDB primarily provides launcher hero artwork with non-wallpaper aspect ratios.
