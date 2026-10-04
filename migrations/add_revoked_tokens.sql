-- Run once on existing databases before starting the updated API.
BEGIN;
CREATE TABLE IF NOT EXISTS revoked_tokens (
    jti text PRIMARY KEY,
    expires_at timestamptz NOT NULL
);
COMMIT;
