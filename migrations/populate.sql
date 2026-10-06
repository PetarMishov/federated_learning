-- Demo data for migrations/schema.sql. Run after creating the schema:
-- psql "$DATABASE_URL" -f migrations/populate.sql
-- Login: username = demo, password = demo-password
-- Password uses Argon2id with the same parameters as Rust Argon2::default().
\set ON_ERROR_STOP on

BEGIN;

INSERT INTO users (username, password_hash)
VALUES (
    'demo',
    '$argon2id$v=19$m=19456,t=2,p=1$v2IEKdUJ6RrtwyNZukpOOw$ffLcHWvpVrdjY/KyDvsTAoC6WQaMFdwb+zDnqlX5BZ4'
)
ON CONFLICT (username) DO NOTHING;

-- The demo user owns these organizations and must also be a member.
-- Organization creation and membership insertion share this transaction to
-- satisfy the deferred owner-membership foreign key.
DO $$
DECLARE
    demo_user_id integer;
    demo_org_id integer;
    org_name text;
BEGIN
    SELECT id INTO demo_user_id FROM users WHERE username = 'demo';

    FOREACH org_name IN ARRAY ARRAY['Central Hospital', 'Research Lab', 'Medical Network']
    LOOP
        SELECT id INTO demo_org_id FROM organizations
        WHERE name = org_name AND owner_user_id = demo_user_id
        ORDER BY id LIMIT 1;

        IF demo_org_id IS NULL THEN
            INSERT INTO organizations (name, owner_user_id)
            VALUES (org_name, demo_user_id)
            RETURNING id INTO demo_org_id;
        END IF;

        INSERT INTO user_organization (user_id, org_id)
        VALUES (demo_user_id, demo_org_id)
        ON CONFLICT (user_id, org_id) DO NOTHING;

        INSERT INTO projects (
            org_id, created_by_user_id, name,
            input_mount_destination, output_mount_destination
        )
        SELECT demo_org_id, demo_user_id, p.name, '/data/input', '/data/output'
        FROM (VALUES ('Model training'), ('Model evaluation')) AS p(name)
        WHERE NOT EXISTS (
            SELECT 1 FROM projects AS existing
            WHERE existing.org_id = demo_org_id AND existing.name = p.name
        );
    END LOOP;
END $$;

INSERT INTO notifications (user_id, title, message, created_at, read_at)
SELECT u.id, n.title, n.message, n.created_at, n.read_at
FROM users AS u
CROSS JOIN (VALUES
    ('Welcome!', 'Your account is ready.',
     TIMESTAMPTZ '2026-10-01 09:00:00+00', TIMESTAMPTZ '2026-10-01 09:15:00+00'),
    ('Organizations ready', 'Your demo organizations are available on the home page.',
     TIMESTAMPTZ '2026-10-03 14:00:00+00', NULL),
    ('Explore your dashboard', 'View your organizations and notifications to get started.',
     TIMESTAMPTZ '2026-10-04 08:30:00+00', NULL)
) AS n(title, message, created_at, read_at)
WHERE u.username = 'demo'
  AND NOT EXISTS (
      SELECT 1 FROM notifications AS existing
      WHERE existing.user_id = u.id AND existing.title = n.title
        AND existing.message = n.message
  );

-- Re-running keeps existing demo data and does not reset an existing password.
COMMIT;
