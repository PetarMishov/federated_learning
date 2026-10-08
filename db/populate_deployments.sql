-- Called by populate_db.sh after publishing the Git commits and immutable refs.
\set ON_ERROR_STOP on
BEGIN;
CREATE TEMP TABLE demo_git_snapshots ON COMMIT DROP AS
SELECT * FROM jsonb_to_recordset(:'demo_snapshots_json'::jsonb)
AS d(project_id integer, snapshot_id integer, owner_id integer, version text, git_commit_sha text);

INSERT INTO snapshots (id, project_id, created_by_user_id, source, git_commit_sha)
OVERRIDING SYSTEM VALUE
SELECT snapshot_id, project_id, owner_id, 'local', git_commit_sha FROM demo_git_snapshots
ON CONFLICT (id) DO NOTHING;
DO $$ BEGIN
    IF EXISTS (
        SELECT 1 FROM demo_git_snapshots d JOIN snapshots s ON s.id = d.snapshot_id
        WHERE s.project_id <> d.project_id OR s.git_commit_sha <> d.git_commit_sha
    ) THEN RAISE EXCEPTION 'Demo Git snapshot metadata conflict'; END IF;
END $$;

-- One snapshot/deployment fixture per demo project, plus a superseded version.
-- Git references have already been published before these records are committed.
DO $$
DECLARE
    owner_id integer;
    p record;
    snapshot_id integer;
    replacement_snapshot_id integer;
    deployment_id integer;
    replacement_id integer;
    collaborator_id integer;
BEGIN
    SELECT id INTO owner_id FROM users WHERE username = 'demo';
    FOR p IN SELECT project.*, o.name AS organization_name
             FROM projects project JOIN organizations o ON o.id = project.org_id
             JOIN demo_git_snapshots demo ON demo.project_id = project.id AND demo.version = 'v1'
             WHERE o.owner_user_id = demo.owner_id AND project.name IN (
                 'Patient risk prediction', 'Medical image classification',
                 'Federated learning benchmark', 'Privacy-preserving model evaluation',
                 'Training algorithm comparison', 'Cross-hospital outcome prediction'
             ) ORDER BY project.id
    LOOP
        SELECT d.snapshot_id INTO STRICT snapshot_id FROM demo_git_snapshots d
        WHERE d.project_id = p.id AND d.version = 'v1';
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
            SELECT d.snapshot_id INTO STRICT replacement_snapshot_id FROM demo_git_snapshots d
            WHERE d.project_id = p.id AND d.version = 'v2';
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
-- Add several runs per project so each project page has a varied deployment list.
-- Reuse each project's real v1 snapshot; every run still belongs to one project.
DO $$
DECLARE
    owner_id integer;
    fixture record;
    deployment_id integer;
    collaborator_id integer;
    deployment_name text;
BEGIN
    SELECT id INTO owner_id FROM users WHERE username = 'demo';
    FOR fixture IN
        SELECT p.id AS project_id, p.org_id, p.name AS project_name,
               o.name AS organization_name, s.id AS snapshot_id,
               examples.label, examples.target_status, examples.joined_count
        FROM (VALUES
            ('Patient risk prediction', 'Awaiting hospital approval', 'pending', 0),
            ('Patient risk prediction', 'Risk model training', 'started', 2),
            ('Patient risk prediction', 'Baseline evaluation', 'finished', 2),
            ('Medical image classification', 'Ready for image training', 'pending', 2),
            ('Medical image classification', 'Image worker failure', 'failed', 2),
            ('Medical image classification', 'Cancelled image experiment', 'cancelled', 0),
            ('Federated learning benchmark', 'Benchmark in progress', 'started', 2),
            ('Federated learning benchmark', 'Completed benchmark', 'finished', 2),
            ('Federated learning benchmark', 'Cancelled benchmark', 'cancelled', 2),
            ('Privacy-preserving model evaluation', 'Awaiting evaluation partner', 'pending', 1),
            ('Privacy-preserving model evaluation', 'Privacy evaluation in progress', 'started', 2),
            ('Privacy-preserving model evaluation', 'Completed privacy evaluation', 'finished', 2),
            ('Training algorithm comparison', 'Awaiting comparison approval', 'pending', 0),
            ('Training algorithm comparison', 'Comparison worker failure', 'failed', 2),
            ('Training algorithm comparison', 'Completed algorithm comparison', 'finished', 2),
            ('Cross-hospital outcome prediction', 'Awaiting another hospital', 'pending', 1),
            ('Cross-hospital outcome prediction', 'Outcome training in progress', 'started', 2),
            ('Cross-hospital outcome prediction', 'Cancelled outcome experiment', 'cancelled', 2)
        ) AS examples(project_name, label, target_status, joined_count)
        JOIN projects p ON p.name = examples.project_name
        JOIN organizations o ON o.id = p.org_id AND o.owner_user_id = owner_id
        JOIN demo_git_snapshots d ON d.project_id = p.id AND d.version = 'v1'
        JOIN snapshots s ON s.id = d.snapshot_id AND s.project_id = p.id
        ORDER BY p.id, examples.label
    LOOP
        deployment_name := 'Demo: ' || fixture.label;
        IF EXISTS (SELECT 1 FROM deployment_runs
                   WHERE project_id = fixture.project_id AND name = deployment_name) THEN
            CONTINUE;
        END IF;
        deployment_id := publish_deployment(owner_id, fixture.project_id,
            fixture.snapshot_id, deployment_name);
        SELECT u.id INTO collaborator_id FROM users u
        JOIN user_organization m ON m.user_id = u.id
        WHERE m.org_id = fixture.org_id AND u.username = CASE fixture.organization_name
            WHEN 'Central Hospital' THEN 'alice'
            WHEN 'Research Lab' THEN 'charlie'
            ELSE 'diana' END;

        IF fixture.joined_count >= 1 THEN
            PERFORM join_deployment(owner_id, deployment_id, '/demo/input/demo', '/demo/output/demo');
        END IF;
        IF fixture.joined_count >= 2 THEN
            PERFORM join_deployment(collaborator_id, deployment_id, '/demo/input/member', '/demo/output/member');
        END IF;
        IF fixture.target_status = 'pending' THEN
            CONTINUE;
        END IF;
        IF fixture.joined_count >= 2 THEN
            PERFORM start_deployment(owner_id, deployment_id);
            UPDATE run_participants SET execution_status = 'running'
            WHERE run_id = deployment_id AND participation_status = 'joined';
        END IF;
        IF fixture.target_status = 'cancelled' THEN
            PERFORM cancel_deployment(owner_id, deployment_id, 'Demo experiment cancelled by its coordinator');
            UPDATE run_participants SET execution_status = 'stopped', stopped_at = clock_timestamp()
            WHERE run_id = deployment_id AND participation_status = 'joined';
        ELSIF fixture.target_status IN ('finished', 'failed') THEN
            UPDATE run_participants SET execution_status = CASE fixture.target_status
                WHEN 'finished' THEN 'completed'::execution_status ELSE 'failed'::execution_status END
            WHERE run_id = deployment_id AND participation_status = 'joined';
            UPDATE deployment_runs SET status = fixture.target_status::run_status,
                ended_at = clock_timestamp(), terminal_reason = CASE fixture.target_status
                    WHEN 'failed' THEN 'Demo worker failed while training the model' ELSE NULL END
            WHERE id = deployment_id;
        END IF;
    END LOOP;
END $$;

COMMIT;
