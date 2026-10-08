-- Demo data for db/schema.sql. Run after creating the schema:
-- ./db/scripts/populate_db.sh (also prepares retained Git snapshots)
-- Login: username = demo, password = demo-password
-- Additional demo users: alice, bob, charlie, diana, eric (password = demo-password).
-- Password uses Argon2id with the same parameters as Rust Argon2::default().
\set ON_ERROR_STOP on

BEGIN;

INSERT INTO users (username, password_hash)
VALUES (
    'demo',
    '$argon2id$v=19$m=19456,t=2,p=1$v2IEKdUJ6RrtwyNZukpOOw$ffLcHWvpVrdjY/KyDvsTAoC6WQaMFdwb+zDnqlX5BZ4'
)
ON CONFLICT (username) DO NOTHING;

INSERT INTO users (username, password_hash)
SELECT u.username,
    '$argon2id$v=19$m=19456,t=2,p=1$v2IEKdUJ6RrtwyNZukpOOw$ffLcHWvpVrdjY/KyDvsTAoC6WQaMFdwb+zDnqlX5BZ4'
FROM (VALUES ('alice'), ('bob'), ('charlie'), ('diana'), ('eric')) AS u(username)
ON CONFLICT (username) DO NOTHING;

-- The demo user owns these organizations and must also be a member.
-- Organization creation and membership insertion share this transaction to
-- satisfy the deferred owner-membership foreign key.
DO $$
DECLARE
    demo_user_id integer;
    demo_org_id integer;
    org_name text;
    coordinator_role_id integer;
    participant_role_id integer;
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

        INSERT INTO roles (org_id, name) VALUES
            (demo_org_id, 'Demo coordinator'), (demo_org_id, 'Demo participant')
        ON CONFLICT (org_id, name) DO NOTHING;
        SELECT id INTO coordinator_role_id FROM roles WHERE org_id = demo_org_id AND name = 'Demo coordinator';
        SELECT id INTO participant_role_id FROM roles WHERE org_id = demo_org_id AND name = 'Demo participant';
        INSERT INTO role_permission (role_id, perm_id)
        SELECT coordinator_role_id, id FROM permissions WHERE name = 'edit_roles'
        ON CONFLICT DO NOTHING;

        -- Each organization has its own member list; demo belongs to all three.
        INSERT INTO user_organization (user_id, org_id, role_id)
        SELECT u.id, demo_org_id, CASE WHEN u.username IN ('alice', 'diana') THEN coordinator_role_id ELSE participant_role_id END
        FROM (VALUES
            ('Central Hospital', 'alice'),
            ('Central Hospital', 'bob'),
            ('Research Lab', 'charlie'),
            ('Medical Network', 'diana'),
            ('Medical Network', 'eric')
        ) AS membership(organization_name, username)
        JOIN users AS u ON u.username = membership.username
        WHERE membership.organization_name = org_name
        ON CONFLICT (user_id, org_id) DO UPDATE SET role_id = EXCLUDED.role_id
        WHERE user_organization.role_id IS NULL;

        INSERT INTO projects (
            org_id, created_by_user_id, name,
            input_mount_destination, output_mount_destination
        )
        SELECT demo_org_id, demo_user_id, p.name, '/data/input', '/data/output'
        -- Each project row has one org_id and belongs to exactly one organization.
        FROM (VALUES
            ('Central Hospital', 'Patient risk prediction'),
            ('Central Hospital', 'Medical image classification'),
            ('Research Lab', 'Federated learning benchmark'),
            ('Research Lab', 'Privacy-preserving model evaluation'),
            ('Research Lab', 'Training algorithm comparison'),
            ('Medical Network', 'Cross-hospital outcome prediction')
        ) AS p(organization_name, name)
        WHERE p.organization_name = org_name
          AND NOT EXISTS (
            SELECT 1 FROM projects AS existing
            WHERE existing.org_id = demo_org_id AND existing.name = p.name
        );
        INSERT INTO role_project_permission (org_id, role_id, project_id, perm_id)
        SELECT demo_org_id, r.id, p.id, perm.id
        FROM roles r JOIN projects p ON p.org_id = r.org_id CROSS JOIN permissions perm
        WHERE r.org_id = demo_org_id AND perm.scope = 'project'
          AND (r.id = coordinator_role_id OR (r.id = participant_role_id AND perm.name = 'participate_in_deployment'))
        ON CONFLICT DO NOTHING;
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

COMMIT;
