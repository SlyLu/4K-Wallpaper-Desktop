# ADR 0010: Add Wikimedia Commons as the first V2 provider

- Status: Accepted
- Date: 2026-08-28

## Context

V2 requires at least one additional maintained provider using an official API. The provider must participate in the same aggregated search and refresh path as Wallhaven, preserve provenance, and avoid embedding a shared secret in the desktop binary.

## Decision

- Wikimedia Commons is the first new built-in online provider.
- The adapter uses the official MediaWiki Action API at `https://commons.wikimedia.org/w/api.php`; it does not scrape web pages.
- Search uses File namespace results and requests `imageinfo` for original URL, thumbnail URL, MIME type, dimensions, size, SHA-1, uploader, and selected license metadata.
- Only supported raster images meeting the active minimum resolution and SFW-oriented query are returned. SVG, PDF, audio, and video files are excluded from the wallpaper result set.
- The adapter sends an identifying User-Agent, uses bounded pages and timeouts, and remains independently disableable.
- Creator, license name, license URL, source page, and provider file identifier are retained in provider-source records. The UI must expose attribution before this provider is enabled in a production release.
- Wallhaven and LocalProvider remain enabled. Aggregated operations query all enabled compatible providers; Wikimedia Commons is not a replacement or a user-selected global default.

## Alternatives considered

- Unsplash and Pexels provide strong photography catalogs, but production use requires application credentials and additional API-specific usage rules. They remain candidates after credential and policy review.
- Web scraping was rejected because it is less stable and conflicts with the V2 provider boundary.

## Consequences

The first multi-provider implementation can work without distributing an API key. License metadata becomes a required part of the provider-source schema and detail UI rather than optional display text.

## References

- https://www.mediawiki.org/wiki/API:Action_API
- https://www.mediawiki.org/wiki/API:Search
- https://www.mediawiki.org/wiki/API:Imageinfo
- https://commons.wikimedia.org/wiki/Commons:Reusing_content_outside_Wikimedia
