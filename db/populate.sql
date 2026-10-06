-- Demo data for db/schema.sql. Run after creating the schema:
-- ./db/scripts/populate_db.sh (also prepares real snapshot files)
-- Login: username = demo, password = demo-password
-- Additional demo users: alice, bob, charlie, diana, eric (password = demo-password).
-- Password uses Argon2id with the same parameters as Rust Argon2::default().
\set ON_ERROR_STOP on

BEGIN;

-- Provided by populate_db.sh after writing the matching snapshot archives.
CREATE TEMP TABLE demo_snapshot_hashes (v1 text, v2 text) ON COMMIT DROP;
INSERT INTO demo_snapshot_hashes VALUES (:'snapshot_v1_sha256', :'snapshot_v2_sha256');

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

-- One snapshot/deployment fixture per demo project, plus a superseded version.
-- These are demonstration records, not real executions or repository imports.
DO $$
DECLARE
    owner_id integer;
    p record;
    snapshot_id integer;
    replacement_snapshot_id integer;
    deployment_id integer;
    replacement_id integer;
    collaborator_id integer;
    storage_prefix text;
    v1_hash text;
    v2_hash text;
BEGIN
    SELECT id INTO owner_id FROM users WHERE username = 'demo';
    SELECT v1, v2 INTO v1_hash, v2_hash FROM demo_snapshot_hashes;
    FOR p IN SELECT project.*, o.name AS organization_name
             FROM projects project JOIN organizations o ON o.id = project.org_id
             WHERE o.owner_user_id = owner_id AND project.name IN (
                 'Patient risk prediction', 'Medical image classification',
                 'Federated learning benchmark', 'Privacy-preserving model evaluation',
                 'Training algorithm comparison', 'Cross-hospital outcome prediction'
             ) ORDER BY project.id
    LOOP
        storage_prefix := 'demo/' || regexp_replace(lower(p.organization_name), '[^a-z0-9]+', '-', 'g')
            || '/' || regexp_replace(lower(p.name), '[^a-z0-9]+', '-', 'g');
        INSERT INTO snapshots (project_id, created_by_user_id, source, storage_key, content_sha256)
        VALUES (p.id, owner_id, 'local', storage_prefix || '/v1.tar', v1_hash)
        ON CONFLICT (storage_key) DO NOTHING;
        SELECT id INTO snapshot_id FROM snapshots WHERE storage_key = storage_prefix || '/v1.tar';
        IF EXISTS (SELECT 1 FROM snapshots WHERE id = snapshot_id AND content_sha256 <> v1_hash) THEN
            RAISE EXCEPTION 'Demo snapshot hash does not match its files';
        END IF;
        IF EXISTS (SELECT 1 FROM deployment_runs WHERE project_id = p.id AND name = 'Demo: ' || p.name) THEN
            CONTINUE;
        END IF;
        deployment_id := publish_deployment(owner_id, p.id, snapshot_id, 'Demo: ' || p.name);
        SELECT u.id INTO collaborator_id FROM users u JOIN user_organization m ON m.user_id = u.id
        WHERE m.org_id = p.org_id AND u.username = CASE p.organization_name
            WHEN 'Central Hospital' THEN 'alice' WHEN 'Research Lab' THEN 'charlie' ELSE 'diana' END;

        IF p.name = 'Training algorithm comparison' THEN
            PERFORM cancel_deployment(owner_id, deployment_id, 'Demo cancelled before starting');
            CONTINUE;
        END IF;
        PERFORM join_deployment(owner_id, deployment_id, '/demo/input/demo', '/demo/output/demo');
        PERFORM join_deployment(collaborator_id, deployment_id, '/demo/input/member', '/demo/output/member');
        IF p.name = 'Patient risk prediction' THEN
            INSERT INTO snapshots (project_id, created_by_user_id, source, storage_key, content_sha256)
            VALUES (p.id, owner_id, 'local', storage_prefix || '/v2.tar', v2_hash)
            ON CONFLICT (storage_key) DO NOTHING;
            SELECT id INTO replacement_snapshot_id FROM snapshots WHERE storage_key = storage_prefix || '/v2.tar';
            IF EXISTS (SELECT 1 FROM snapshots WHERE id = replacement_snapshot_id AND content_sha256 <> v2_hash) THEN
                RAISE EXCEPTION 'Replacement snapshot hash does not match its files';
            END IF;
            replacement_id := publish_deployment(owner_id, p.id, replacement_snapshot_id,
                'Demo: Patient risk prediction update', 'Changed training rounds from 3 to 5.', deployment_id);
            -- Demonstrate leaving and rejoining while pending without retaining acceptance.
            PERFORM join_deployment(collaborator_id, replacement_id, '/demo/input/member', '/demo/output/member');
            PERFORM leave_deployment(collaborator_id, replacement_id);
            CONTINUE;
        END IF;
        PERFORM start_deployment(owner_id, deployment_id);
        IF p.name = 'Federated learning benchmark' THEN
            UPDATE run_participants SET execution_status = 'completed' WHERE run_id = deployment_id AND participation_status = 'joined';
            UPDATE deployment_runs SET status = 'finished', ended_at = clock_timestamp() WHERE id = deployment_id;
        ELSIF p.name = 'Privacy-preserving model evaluation' THEN
            UPDATE run_participants SET execution_status = 'failed' WHERE run_id = deployment_id AND participation_status = 'joined';
            UPDATE deployment_runs SET status = 'failed', ended_at = clock_timestamp(),
                terminal_reason = 'Demo worker error: example execution failed' WHERE id = deployment_id;
        ELSIF p.name = 'Cross-hospital outcome prediction' THEN
            PERFORM leave_deployment(collaborator_id, deployment_id);
            -- One participant remains: still started. Only the last departure cancels.
            PERFORM leave_deployment(owner_id, deployment_id);
        ELSE
            UPDATE run_participants SET execution_status = 'running' WHERE run_id = deployment_id AND participation_status = 'joined';
        END IF;
    END LOOP;
END $$;

-- Re-running keeps existing demo data and does not reset passwords or deployments.
COMMIT;
