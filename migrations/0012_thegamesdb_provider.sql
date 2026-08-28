-- TheGamesDB needs a user-owned API key, so it remains disabled until configured explicitly.
INSERT OR IGNORE INTO provider_config(provider, enabled, updated_at) VALUES
    ('thegamesdb', 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

INSERT OR IGNORE INTO provider_health(provider, updated_at)
VALUES ('thegamesdb', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

INSERT OR IGNORE INTO provider_sync_state(provider, updated_at)
VALUES ('thegamesdb', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
