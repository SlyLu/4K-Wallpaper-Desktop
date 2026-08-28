/** Keeps provider display names consistent without leaking adapter identifiers into templates. */
export function providerLabel(provider: string): string {
  return {
    wallhaven: "Wallhaven",
    wikimedia_commons: "Wikimedia Commons",
    openverse: "Openverse",
    art_institute_chicago: "Art Institute of Chicago",
    thegamesdb: "TheGamesDB",
    local: "本地图库",
  }[provider] ?? provider;
}
